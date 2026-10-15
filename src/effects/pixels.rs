//! Группа «Сортировка и пиксели»: pixel sort, ordered dithering, bit crush.

use crate::core::effect::{Effect, EffectCtx, EffectGroup};
use crate::core::error::GlitchError;
use crate::core::params::ParamSpec;
use crate::core::seed::Rng;
use rayon::prelude::*;

const TAG_SORT: u64 = 0x5053_4F52_5400_0001;
const TAG_DITHER: u64 = 0x4449_5448_4552_0001;
const TAG_CRUSH: u64 = 0x4249_5443_5255_5301;

/// Классические матрицы Байера 4x4 и 8x8 (0..N*N-1, нормализуются делением на N*N).
const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

const BAYER8: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

#[inline]
fn lum(p: [u8; 4]) -> u32 {
    (77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32) >> 8
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

#[inline]
fn hash01(x: u32, y: u32, salt: u32) -> f32 {
    let h = hash_u32(x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ salt);
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Сортирует максимальные пробеги подряд идущих «ярких» пикселей в одной линии.
/// `buf` — упакованный RGBA, длина кратна 4. Пробеги длиннее `max_len` не трогаются.
fn sort_line(buf: &mut [u8], thr: u32, max_len: usize, desc: bool, jitter: f32, rng: &mut Rng) {
    let n = buf.len() / 4;
    if n < 2 {
        return;
    }
    let mut px: Vec<[u8; 4]> = Vec::with_capacity(n);
    for c in buf.chunks_exact(4) {
        px.push([c[0], c[1], c[2], c[3]]);
    }

    let mut i = 0usize;
    while i < n {
        if lum(px[i]) <= thr {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < n && lum(px[i]) > thr {
            if jitter > 0.0 && rng.chance(jitter) {
                break;
            }
            i += 1;
        }
        let end = i;
        if end - start > 1 && end - start <= max_len {
            let seg = &mut px[start..end];
            if desc {
                seg.sort_by_key(|a| u32::MAX - lum(*a));
            } else {
                seg.sort_by_key(|a| lum(*a));
            }
        }
    }

    for (i, c) in buf.chunks_exact_mut(4).enumerate() {
        c.copy_from_slice(&px[i]);
    }
}

pub struct PixelSort;

impl Effect for PixelSort {
    fn id(&self) -> &'static str {
        "pixel_sort"
    }

    fn name(&self) -> &'static str {
        "Pixel Sort"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Pixels
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 5] = [
            ParamSpec::new("amount", "Обработано строк, %", 0.0, 100.0, 70.0, 1.0),
            ParamSpec::new("threshold", "Порог яркости", 0.0, 255.0, 128.0, 1.0),
            ParamSpec::new("length", "Макс. длина пробега, px", 1.0, 500.0, 90.0, 1.0),
            ParamSpec::new("direction", "Направление", 0.0, 3.0, 0.0, 1.0),
            ParamSpec::new("jitter", "Дрожание границ", 0.0, 100.0, 20.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Направление: 0 — строки ↑, 1 — столбцы ↑, 2 — строки ↓, 3 — столбцы ↓. \
         amount зависит от Chaos; порог, длина, направление и дрожание — фиксированные."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let prob = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        if prob <= 0.0 {
            return Ok(());
        }
        let thr = ctx.raw(1).clamp(0.0, 255.0) as u32;
        let max_len = ctx.raw(2).round().clamp(1.0, 500.0) as usize;
        let dir = ctx.raw(3).round().clamp(0.0, 3.0) as u8;
        let jitter = (ctx.raw(4) / 100.0).clamp(0.0, 1.0);
        let vertical = matches!(dir, 1 | 3);
        let desc = matches!(dir, 2 | 3);

        let count = if vertical { w } else { h };
        let mut sel: Vec<(bool, u64)> = Vec::with_capacity(count);
        for _ in 0..count {
            sel.push((ctx.rng.chance(prob), ctx.rng.next_u64_for_tag(TAG_SORT)));
        }

        if vertical {
            for (x, (on, seed)) in sel.iter().enumerate() {
                if !*on {
                    continue;
                }
                let xi = x as i64;
                let mut buf = vec![0u8; h * 4];
                for y in 0..h as i64 {
                    if let Some(p) = ctx.frame.get(xi, y) {
                        let o = y as usize * 4;
                        buf[o..o + 4].copy_from_slice(&p);
                    }
                }
                let mut rng = Rng::new(*seed);
                sort_line(&mut buf, thr, max_len, desc, jitter, &mut rng);
                for y in 0..h as i64 {
                    let o = y as usize * 4;
                    ctx.frame
                        .set(xi, y, [buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
                }
            }
        } else {
            let stride = w * 4;
            let data = &mut ctx.frame.data;
            data.par_chunks_mut(stride)
                .enumerate()
                .for_each(|(y, row)| {
                    let (on, seed) = sel[y];
                    if !on {
                        return;
                    }
                    let mut rng = Rng::new(seed);
                    sort_line(row, thr, max_len, desc, jitter, &mut rng);
                });
        }
        Ok(())
    }
}

pub struct Dither;

impl Effect for Dither {
    fn id(&self) -> &'static str {
        "dither"
    }

    fn name(&self) -> &'static str {
        "Dither"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Pixels
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 40.0, 1.0),
            ParamSpec::new("pattern", "Узор", 0.0, 2.0, 0.0, 1.0),
            ParamSpec::new("levels", "Уровней на канал", 2.0, 8.0, 2.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Узор: 0 — Байер 4x4, 1 — Байер 8x8, 2 — хэш (blue-noise). \
         amount зависит от Chaos; узор и число уровней — фиксированные."
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
        let pattern = ctx.raw(1).round().clamp(0.0, 2.0) as u8;
        let levels = ctx.raw(2).round().clamp(2.0, 8.0) as i32;
        let scale = (levels - 1) as f32;
        let salt = ctx.rng.next_u64_for_tag(TAG_DITHER) as u32;
        let stride = w * 4;

        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..w {
                    let o = x * 4;
                    for c in 0..3 {
                        let v = row[o + c] as f32;
                        let t = match pattern {
                            0 => (BAYER4[y & 3][x & 3] as f32 + 0.5) / 16.0,
                            1 => (BAYER8[y & 7][x & 7] as f32 + 0.5) / 64.0,
                            _ => hash01(x as u32, y as u32, salt),
                        };
                        let q = (((v / 255.0) * scale + (t - 0.5)).round()).clamp(0.0, scale)
                            / scale
                            * 255.0;
                        row[o + c] = (v + (q - v) * amount).round().clamp(0.0, 255.0) as u8;
                    }
                }
            });
        Ok(())
    }
}

pub struct BitCrush;

impl Effect for BitCrush {
    fn id(&self) -> &'static str {
        "bit_crush"
    }

    fn name(&self) -> &'static str {
        "Bit Crush"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Pixels
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("bits", "Бит на канал", 1.0, 8.0, 4.0, 1.0),
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 60.0, 1.0),
            ParamSpec::new("dither", "Дизеринг", 0.0, 100.0, 20.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Число значимых бит на канал; dither добавляет шум перед усечением. \
         amount и dither зависят от Chaos; глубина бит — фиксированная."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amount = (ctx.v(1) / 100.0).clamp(0.0, 1.0);
        if amount <= 0.0 {
            return Ok(());
        }
        let bits = ctx.raw(0).round().clamp(1.0, 8.0) as u32;
        let dither = (ctx.raw(2) / 100.0).clamp(0.0, 1.0);
        let shift = 8 - bits;
        let mask = 0xFFu32 << shift;
        let step = 1u32 << shift;

        let mut seeds: Vec<u64> = Vec::with_capacity(h);
        for _ in 0..h {
            seeds.push(ctx.rng.next_u64_for_tag(TAG_CRUSH));
        }

        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let mut rng = Rng::new(seeds[y]);
                let seeded = dither > 0.0;
                for x in 0..w {
                    let o = x * 4;
                    for c in 0..3 {
                        let v = row[o + c];
                        let n = if seeded {
                            rng.range(0.0, step as f32) as u32
                        } else {
                            0
                        };
                        let q = ((v as u32 + n).min(255) & mask) as u8;
                        row[o + c] = (v as f32 + (q as f32 - v as f32) * amount)
                            .round()
                            .clamp(0.0, 255.0) as u8;
                    }
                }
            });
        Ok(())
    }
}

pub fn effects() -> Vec<Box<dyn Effect>> {
    vec![Box::new(PixelSort), Box::new(Dither), Box::new(BitCrush)]
}
