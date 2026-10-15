//! Группа «Артефакты и порча данных»: jpeg-давление, порча байт, шум, ТВ-помехи,
//! зерно, строки развёртки, CRT, bloom и виньетка.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageReader};
use rayon::prelude::*;

use crate::core::effect::{Effect, EffectCtx, EffectGroup};
use crate::core::error::GlitchError;
use crate::core::frame::Frame;
use crate::core::params::ParamSpec;
use crate::core::seed::Rng;

const TAG_NOISE: u64 = 0x4E4F_4953_4500_0001;
const TAG_GRAIN: u64 = 0x4752_4149_4E00_0001;
const TAG_STATIC: u64 = 0x5354_4154_4943_0001;

#[inline]
fn lum_bytes(r: u8, g: u8, b: u8) -> u32 {
    (77 * r as u32 + 150 * g as u32 + 29 * b as u32) >> 8
}

#[inline]
fn hash_u32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// Знаковый шум -127..=127 из координат блока и соли (соль берётся из ctx.rng).
#[inline]
fn noise_at(bx: u32, by: u32, ch: u32, salt: u32) -> i32 {
    let h = hash_u32(
        bx.wrapping_mul(0x9E37_79B1)
            ^ by.wrapping_mul(0x85EB_CA77)
            ^ ch.wrapping_mul(0xC2B2_AE3D)
            ^ salt,
    );
    (h % 255) as i32 - 127
}

// ---------------------------------------------------------------------------
// JPEG Crush
// ---------------------------------------------------------------------------

/// Кодирует RGB-буфер в JPEG в памяти и декодирует назад. `None` при любой ошибке.
fn jpeg_roundtrip(rgb: &[u8], w: u32, h: u32, q: u8) -> Option<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut enc = JpegEncoder::new_with_quality(Cursor::new(&mut buf), q);
        enc.encode(rgb, w, h, ExtendedColorType::Rgb8).ok()?;
    }
    let reader = ImageReader::new(Cursor::new(&buf))
        .with_guessed_format()
        .ok()?;
    let img = reader.decode().ok()?;
    let rgba = img.to_rgba8();
    let raw = rgba.into_raw();
    let mut out = Vec::with_capacity(rgb.len());
    for px in raw.chunks_exact(4) {
        out.push(px[0]);
        out.push(px[1]);
        out.push(px[2]);
    }
    Some(out)
}

pub struct JpegCrush;

impl Effect for JpegCrush {
    fn id(&self) -> &'static str {
        "jpeg_crush"
    }

    fn name(&self) -> &'static str {
        "JPEG Crush"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Artifacts
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("passes", "Проходов", 1.0, 12.0, 4.0, 1.0),
            ParamSpec::new("quality", "Качество JPEG", 1.0, 100.0, 15.0, 1.0),
            ParamSpec::new("chroma", "Сохранение хромы, %", 0.0, 100.0, 80.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Гоняет кадр через JPEG-кодек N раз. Все параметры фиксированные (не зависят от Chaos): \
         число проходов, качество и доля сохранённой хромы."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width;
        let h = ctx.frame.height;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let passes = ctx.raw(0).round().clamp(1.0, 12.0) as usize;
        let q = ctx.raw(1).round().clamp(1.0, 100.0) as u8;
        let keep = (ctx.raw(2) / 100.0).clamp(0.0, 1.0);

        let mut rgb: Vec<u8> = Vec::with_capacity(w as usize * h as usize * 3);
        for px in ctx.frame.data.chunks_exact(4) {
            rgb.push(px[0]);
            rgb.push(px[1]);
            rgb.push(px[2]);
        }

        for _ in 0..passes {
            match jpeg_roundtrip(&rgb, w, h, q) {
                Some(next) => rgb = next,
                // Любая ошибка кодека — кадр остаётся нетронутым, пайплайн не рвётся.
                None => return Ok(()),
            }
        }

        let n = (w as usize) * (h as usize);
        for i in 0..n {
            let o = i * 4;
            let s = i * 3;
            if keep < 1.0 {
                let r = rgb[s] as f32;
                let g = rgb[s + 1] as f32;
                let b = rgb[s + 2] as f32;
                let l = ((77.0 * r + 150.0 * g + 29.0 * b) / 256.0).round();
                ctx.frame.data[o] = (l + (r - l) * keep).clamp(0.0, 255.0) as u8;
                ctx.frame.data[o + 1] = (l + (g - l) * keep).clamp(0.0, 255.0) as u8;
                ctx.frame.data[o + 2] = (l + (b - l) * keep).clamp(0.0, 255.0) as u8;
            } else {
                ctx.frame.data[o] = rgb[s];
                ctx.frame.data[o + 1] = rgb[s + 1];
                ctx.frame.data[o + 2] = rgb[s + 2];
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Databend
// ---------------------------------------------------------------------------

pub struct Databend;

impl Effect for Databend {
    fn id(&self) -> &'static str {
        "databend"
    }

    fn name(&self) -> &'static str {
        "Databend"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Artifacts
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 4] = [
            ParamSpec::new("level", "Сила порчи", 0.0, 100.0, 50.0, 1.0),
            ParamSpec::new("width", "Ширина пятна, байт", 1.0, 16.0, 3.0, 1.0),
            ParamSpec::new("repeat", "Повторов", 1.0, 8.0, 2.0, 1.0),
            ParamSpec::new("seed_spread", "Разброс", 0.0, 100.0, 50.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Портит сырые байты RGBA: XOR, сдвиг или перезапись в выбранных регионах. \
         level зависит от Chaos; размер, число пятен и разброс — фиксированные. \
         Альфа испорченных пикселей принудительно ставится в 255."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let npx = ctx.frame.pixel_count();
        if npx == 0 {
            return Ok(());
        }
        let level = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        if level <= 0.0 {
            return Ok(());
        }
        let width = ctx.raw(1).round().clamp(1.0, 16.0) as usize;
        let repeat = ctx.raw(2).round().clamp(1.0, 8.0) as usize;
        let spread = (ctx.raw(3) / 100.0).clamp(0.0, 1.0);
        let npx32 = npx.min(u32::MAX as usize) as u32;

        // План порчи строится заранее из ctx.rng, затем применяется к буферу —
        // так один и тот же seed даёт идентичный результат.
        let mut ops: Vec<(usize, u8, u8)> = Vec::new();
        for _ in 0..repeat {
            let start = ctx.rng.below(npx32) as usize;
            let span = (width * (1 + ctx.rng.below(8) as usize)).max(1);
            let end = (start + span).min(npx);
            for p in start..end {
                for c in 0..3usize {
                    let mode = ctx.rng.below(3) as u8;
                    let val = ctx.rng.below(256) as u8;
                    ops.push((p * 4 + c, mode, val));
                }
            }
        }
        let speckles = ((spread * 400.0) as usize).min(4000);
        for _ in 0..speckles {
            let p = ctx.rng.below(npx32) as usize;
            let c = ctx.rng.below(3) as usize;
            let mode = ctx.rng.below(3) as u8;
            let val = ctx.rng.below(256) as u8;
            ops.push((p * 4 + c, mode, val));
        }

        let bytes = &mut ctx.frame.data;
        for &(idx, mode, val) in &ops {
            if idx >= bytes.len() {
                continue;
            }
            let orig = bytes[idx];
            let target = match mode {
                0 => orig ^ val,
                1 => orig.rotate_left((val & 7) as u32),
                _ => val,
            };
            bytes[idx] = (orig as f32 + (target as f32 - orig as f32) * level)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
        // RGB испорчен на пикселях с прозрачной альфой может «исчезнуть»: возвращаем непрозрачность.
        for &(idx, _, _) in &ops {
            let a = (idx / 4) * 4 + 3;
            if a < bytes.len() {
                bytes[a] = 255;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Noise
// ---------------------------------------------------------------------------

pub struct Noise;

impl Effect for Noise {
    fn id(&self) -> &'static str {
        "noise"
    }

    fn name(&self) -> &'static str {
        "Noise"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Artifacts
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 40.0, 1.0),
            ParamSpec::new("mono", "Моно, %", 0.0, 100.0, 0.0, 1.0),
            ParamSpec::new("grain_size", "Размер блока, px", 1.0, 8.0, 1.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Попиксельный шум; grain_size>1 объединяет соседние пиксели в блоки. \
         amount зависит от Chaos; моно и размер блока — фиксированные. \
         Шум детерминирован солью из ctx.rng."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amount = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        if amount <= 0.0 {
            return Ok(());
        }
        let mono = (ctx.raw(1) / 100.0).clamp(0.0, 1.0);
        let gs = ctx.raw(2).round().clamp(1.0, 8.0) as u32;
        let salt = ctx.rng.next_u64_for_tag(TAG_NOISE) as u32;
        let stride = w * 4;

        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let by = y as u32 / gs;
                for x in 0..w {
                    let bx = x as u32 / gs;
                    let nr = noise_at(bx, by, 0, salt) as f32;
                    let ng = noise_at(bx, by, 1, salt) as f32;
                    let nb = noise_at(bx, by, 2, salt) as f32;
                    let dg = ng + (nr - ng) * mono;
                    let db = nb + (nr - nb) * mono;
                    let o = x * 4;
                    row[o] = (row[o] as f32 + nr * amount).round().clamp(0.0, 255.0) as u8;
                    row[o + 1] = (row[o + 1] as f32 + dg * amount).round().clamp(0.0, 255.0) as u8;
                    row[o + 2] = (row[o + 2] as f32 + db * amount).round().clamp(0.0, 255.0) as u8;
                }
            });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Static
// ---------------------------------------------------------------------------

pub struct Static;

impl Effect for Static {
    fn id(&self) -> &'static str {
        "static"
    }

    fn name(&self) -> &'static str {
        "Static"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Artifacts
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("amount", "Плотность", 0.0, 100.0, 45.0, 1.0),
            ParamSpec::new("blocks", "Доля строк, %", 0.0, 100.0, 50.0, 1.0),
            ParamSpec::new("length", "Длина полосы, px", 1.0, 64.0, 4.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Полосы телевизионных помех по случайным строкам. amount зависит от Chaos; \
         доля строк и длина полосы — фиксированные."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amount = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        if amount <= 0.0 {
            return Ok(());
        }
        let blocks = (ctx.raw(1) / 100.0).clamp(0.0, 1.0);
        let length = ctx.raw(2).round().clamp(1.0, 64.0) as usize;

        let mut seeds: Vec<u64> = Vec::with_capacity(h);
        for _ in 0..h {
            seeds.push(ctx.rng.next_u64_for_tag(TAG_STATIC));
        }

        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let mut rng = Rng::new(seeds[y]);
                if !rng.chance(blocks) {
                    return;
                }
                let mut x = 0usize;
                while x < w {
                    if rng.chance(amount) {
                        let end = (x + length).min(w);
                        for i in x..end {
                            let o = i * 4;
                            for c in 0..3 {
                                let n = rng.below(256) as f32;
                                let v = row[o + c] as f32;
                                row[o + c] = (v + (n - v) * amount).round().clamp(0.0, 255.0) as u8;
                            }
                            row[o + 3] = 255;
                        }
                    }
                    x += length;
                }
            });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Grain
// ---------------------------------------------------------------------------

pub struct Grain;

impl Effect for Grain {
    fn id(&self) -> &'static str {
        "grain"
    }

    fn name(&self) -> &'static str {
        "Film Grain"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Artifacts
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 35.0, 1.0),
            ParamSpec::new("size", "Размер зерна, px", 1.0, 4.0, 1.0, 1.0),
            ParamSpec::new("luma_only", "Только яркость, %", 0.0, 100.0, 100.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Кинозерно, сильнее в тенях: амплитуда шума падает с ростом яркости. \
         amount зависит от Chaos; размер зерна и luma_only — фиксированные. \
         Шум детерминирован солью из ctx.rng."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amount = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        if amount <= 0.0 {
            return Ok(());
        }
        let size = ctx.raw(1).round().clamp(1.0, 4.0) as u32;
        let luma_only = (ctx.raw(2) / 100.0).clamp(0.0, 1.0);
        let salt = ctx.rng.next_u64_for_tag(TAG_GRAIN) as u32;
        let stride = w * 4;

        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let by = y as u32 / size;
                for x in 0..w {
                    let bx = x as u32 / size;
                    let dr = noise_at(bx, by, 0, salt) as f32;
                    let dg = noise_at(bx, by, 1, salt) as f32;
                    let db = noise_at(bx, by, 2, salt) as f32;
                    let o = x * 4;
                    let l = lum_bytes(row[o], row[o + 1], row[o + 2]) as f32;
                    let modulation = 1.0 - 0.8 * (l / 255.0);
                    let amp = amount * modulation;
                    let dl = (dr + dg + db) / 3.0;
                    let er = dr + (dl - dr) * luma_only;
                    let eg = dg + (dl - dg) * luma_only;
                    let eb = db + (dl - db) * luma_only;
                    row[o] = (row[o] as f32 + er * amp).round().clamp(0.0, 255.0) as u8;
                    row[o + 1] = (row[o + 1] as f32 + eg * amp).round().clamp(0.0, 255.0) as u8;
                    row[o + 2] = (row[o + 2] as f32 + eb * amp).round().clamp(0.0, 255.0) as u8;
                }
            });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Scanlines
// ---------------------------------------------------------------------------

pub struct Scanlines;

impl Effect for Scanlines {
    fn id(&self) -> &'static str {
        "scanlines"
    }

    fn name(&self) -> &'static str {
        "Scanlines"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Artifacts
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 4] = [
            ParamSpec::new("intensity", "Сила", 0.0, 100.0, 40.0, 1.0),
            ParamSpec::new("period", "Период, строк", 1.0, 16.0, 2.0, 1.0),
            ParamSpec::new("thickness", "Скважность, %", 0.0, 100.0, 50.0, 1.0),
            ParamSpec::new("offset", "Сдвиг фазы", 0.0, 16.0, 0.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Затемнение каждой N-й строки. intensity зависит от Chaos; период, скважность и сдвиг — фиксированные."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let intensity = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        if intensity <= 0.0 {
            return Ok(());
        }
        let period = ctx.raw(1).round().clamp(1.0, 16.0) as i64;
        let duty = (ctx.raw(2) / 100.0).clamp(0.0, 1.0);
        let offset = ctx.raw(3).round().max(0.0) as i64;
        let f = 1.0 - intensity;
        let lit = period as f32 * duty;
        let stride = w * 4;

        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let phase = (y as i64 + offset).rem_euclid(period);
                if (phase as f32) < lit {
                    for o in (0..row.len()).step_by(4) {
                        row[o] = (row[o] as f32 * f).round().clamp(0.0, 255.0) as u8;
                        row[o + 1] = (row[o + 1] as f32 * f).round().clamp(0.0, 255.0) as u8;
                        row[o + 2] = (row[o + 2] as f32 * f).round().clamp(0.0, 255.0) as u8;
                    }
                }
            });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CRT
// ---------------------------------------------------------------------------

pub struct Crt;

impl Effect for Crt {
    fn id(&self) -> &'static str {
        "crt"
    }

    fn name(&self) -> &'static str {
        "CRT"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Artifacts
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 4] = [
            ParamSpec::new("intensity", "Сила", 0.0, 100.0, 70.0, 1.0),
            ParamSpec::new("curvature", "Выпуклость", 0.0, 100.0, 30.0, 1.0),
            ParamSpec::new("bleed", "Замазка", 0.0, 100.0, 40.0, 1.0),
            ParamSpec::new("brightness", "Яркость", 0.0, 100.0, 50.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Комбинированный вид кинескопа: бочкообразная дисторсия, горизонтальная замазка, \
         подъём яркости и затемнение углов. intensity зависит от Chaos; \
         выпуклость, замазка и яркость — фиксированные."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let intensity = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        let curv = (ctx.raw(1) / 100.0).clamp(0.0, 1.0) * 0.35;
        let bleed = (ctx.raw(2) / 100.0).clamp(0.0, 1.0);
        let bright = 0.7 + (ctx.raw(3) / 100.0).clamp(0.0, 1.0) * 0.6;

        let src = ctx.frame.clone();
        let stride = w * 4;
        let wm1 = (w as f32 - 1.0).max(1.0);
        let hm1 = (h as f32 - 1.0).max(1.0);
        let mut out = Frame::new(w as u32, h as u32);

        out.data
            .par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let v = (y as f32 / hm1) * 2.0 - 1.0;
                for x in 0..w {
                    let u = (x as f32 / wm1) * 2.0 - 1.0;
                    let r2 = u * u + v * v;
                    let f = 1.0 + curv * r2;
                    // Отображаем искажённую нормированную координату обратно в пиксели источника.
                    let sxp = (u * f * 0.5 + 0.5) * wm1;
                    let syp = (v * f * 0.5 + 0.5) * hm1;
                    let c = src.sample_bilinear(sxp, syp);
                    let edge = (1.0 - intensity * 0.5 * r2).clamp(0.0, 1.0);
                    let o = x * 4;
                    row[o] = (c[0] * bright * edge).clamp(0.0, 255.0) as u8;
                    row[o + 1] = (c[1] * bright * edge).clamp(0.0, 255.0) as u8;
                    row[o + 2] = (c[2] * bright * edge).clamp(0.0, 255.0) as u8;
                    row[o + 3] = src
                        .get(sxp.round() as i64, syp.round() as i64)
                        .map(|p| p[3])
                        .unwrap_or(255);
                }
            });

        let r = (bleed * 5.0).round() as i64;
        if r > 0 {
            let mix = (bleed * 0.5).clamp(0.0, 1.0);
            out.data.par_chunks_mut(stride).for_each(|row| {
                let orig = row.to_vec();
                for x in 0..w {
                    let mut acc = [0f32; 3];
                    let mut cnt = 0f32;
                    let mut k = -r;
                    while k <= r {
                        let xx = x as i64 + k;
                        if (0..w as i64).contains(&xx) {
                            let o = xx as usize * 4;
                            acc[0] += orig[o] as f32;
                            acc[1] += orig[o + 1] as f32;
                            acc[2] += orig[o + 2] as f32;
                            cnt += 1.0;
                        }
                        k += 1;
                    }
                    let o = x * 4;
                    if cnt > 0.0 {
                        for c in 0..3 {
                            let avg = acc[c] / cnt;
                            let val = orig[o + c] as f32 * (1.0 - mix) + avg * mix;
                            row[o + c] = val.round().clamp(0.0, 255.0) as u8;
                        }
                    }
                }
            });
        }

        ctx.frame.copy_from(&out);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Bloom
// ---------------------------------------------------------------------------

/// Горизонтальный box-blur со скользящей суммой, попиксельно по строкам.
fn blur_h(src: &[u8], dst: &mut [u8], w: usize, r: usize) {
    if r == 0 {
        dst.copy_from_slice(src);
        return;
    }
    dst.par_chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
        let base = y * w * 3;
        let srow = &src[base..base + w * 3];
        for c in 0..3 {
            let hi0 = r.min(w - 1);
            let mut sum = 0i32;
            let mut count = 0i32;
            for k in 0..=hi0 {
                sum += srow[k * 3 + c] as i32;
                count += 1;
            }
            for x in 0..w {
                row[x * 3 + c] = (sum / count) as u8;
                let rem = x as i64 - r as i64;
                if rem >= 0 {
                    sum -= srow[rem as usize * 3 + c] as i32;
                    count -= 1;
                }
                let add = x as i64 + r as i64 + 1;
                if add < w as i64 {
                    sum += srow[add as usize * 3 + c] as i32;
                    count += 1;
                }
            }
        }
    });
}

/// Вертикальный box-blur со скользящей суммой, по столбцам.
fn blur_v(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    if r == 0 {
        dst.copy_from_slice(src);
        return;
    }
    let hi0 = r.min(h - 1);
    for x in 0..w {
        for c in 0..3 {
            let mut sum = 0i32;
            let mut count = 0i32;
            for k in 0..=hi0 {
                sum += src[(k * w + x) * 3 + c] as i32;
                count += 1;
            }
            for y in 0..h {
                dst[(y * w + x) * 3 + c] = (sum / count) as u8;
                let rem = y as i64 - r as i64;
                if rem >= 0 {
                    sum -= src[(rem as usize * w + x) * 3 + c] as i32;
                    count -= 1;
                }
                let add = y as i64 + r as i64 + 1;
                if add < h as i64 {
                    sum += src[(add as usize * w + x) * 3 + c] as i32;
                    count += 1;
                }
            }
        }
    }
}

pub struct Bloom;

impl Effect for Bloom {
    fn id(&self) -> &'static str {
        "bloom"
    }

    fn name(&self) -> &'static str {
        "Bloom"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Artifacts
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("threshold", "Порог свечения", 0.0, 255.0, 140.0, 1.0),
            ParamSpec::new("intensity", "Сила", 0.0, 100.0, 40.0, 1.0),
            ParamSpec::new("radius", "Радиус, px", 1.0, 32.0, 8.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Свечение ярких участков: выделяет пиксели выше порога, размывает и добавляет назад. \
         intensity зависит от Chaos; порог и радиус — фиксированные."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let intensity = (ctx.v(1) / 100.0).clamp(0.0, 1.0);
        if intensity <= 0.0 {
            return Ok(());
        }
        let thr = ctx.raw(0).clamp(0.0, 255.0) as u32;
        let r = ctx.raw(2).round().clamp(1.0, 32.0) as usize;
        let stride = w * 4;
        let n = w * h * 3;

        let mut bright = vec![0u8; n];
        {
            let frame = &*ctx.frame;
            bright
                .par_chunks_mut(w * 3)
                .enumerate()
                .for_each(|(y, out)| {
                    let row = frame.row(y as u32);
                    for x in 0..w {
                        let o = x * 4;
                        let l = lum_bytes(row[o], row[o + 1], row[o + 2]);
                        if l > thr {
                            let d = x * 3;
                            out[d] = row[o];
                            out[d + 1] = row[o + 1];
                            out[d + 2] = row[o + 2];
                        }
                    }
                });
        }

        let mut tmp = vec![0u8; n];
        blur_h(&bright, &mut tmp, w, r);
        blur_v(&tmp, &mut bright, w, h, r);

        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..w {
                    let d = x * 3;
                    let o = x * 4;
                    for c in 0..3 {
                        let add = bright[y * w * 3 + d + c] as f32 * intensity;
                        row[o + c] = (row[o + c] as f32 + add).round().clamp(0.0, 255.0) as u8;
                    }
                }
            });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Vignette
// ---------------------------------------------------------------------------

pub struct Vignette;

impl Effect for Vignette {
    fn id(&self) -> &'static str {
        "vignette"
    }

    fn name(&self) -> &'static str {
        "Vignette"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Artifacts
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("intensity", "Сила", 0.0, 100.0, 55.0, 1.0),
            ParamSpec::new("radius", "Радиус, px", 0.0, 200.0, 90.0, 1.0),
            ParamSpec::new("softness", "Мягкость", 0.0, 100.0, 50.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Затемнение по краям кадра, как у старой оптики. \
         intensity зависит от Chaos; радиус и мягкость — фиксированные."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let intensity = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        if intensity <= 0.0 {
            return Ok(());
        }
        let cx = w as f32 * 0.5;
        let cy = h as f32 * 0.5;
        let max_d = (cx * cx + cy * cy).sqrt().max(1.0);
        let radius = ctx.raw(1);
        let soft = (ctx.raw(2) / 100.0).max(0.001);
        let inner = 1.0 - (radius / 100.0);
        let stride = w * 4;

        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let dy = y as f32 - cy;
                for x in 0..w {
                    let dx = x as f32 - cx;
                    let d = (dx * dx + dy * dy).sqrt() / max_d;
                    let t = ((d - inner) / (0.001 + soft)).clamp(0.0, 1.0);
                    let f = 1.0 - intensity * t;
                    let o = x * 4;
                    row[o] = (row[o] as f32 * f).clamp(0.0, 255.0) as u8;
                    row[o + 1] = (row[o + 1] as f32 * f).clamp(0.0, 255.0) as u8;
                    row[o + 2] = (row[o + 2] as f32 * f).clamp(0.0, 255.0) as u8;
                }
            });
        Ok(())
    }
}

pub fn effects() -> Vec<Box<dyn Effect>> {
    vec![
        Box::new(JpegCrush),
        Box::new(Databend),
        Box::new(Noise),
        Box::new(Static),
        Box::new(Grain),
        Box::new(Scanlines),
        Box::new(Crt),
        Box::new(Bloom),
        Box::new(Vignette),
    ]
}
