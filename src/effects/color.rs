//! Группа «Цвет и каналы»: RGB-сдвиг, подмена каналов, поворот тона,
//! соляризация, инверсия, постеризация и растекание цвета.

use crate::core::effect::{Effect, EffectCtx, EffectGroup};
use crate::core::error::GlitchError;
use crate::core::params::ParamSpec;
use rayon::prelude::*;

/// Безопасное значение ползунка: NaN/inf → fallback, иначе зажим в `[lo, hi]`.
/// Обязательно перед любым приведением к целому — иначе inf даёт i64::MAX и переполнение.
#[inline]
fn sane(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

/// Квантование канала к `levels` уровням (2..=16).
#[inline]
fn quantize(v: f32, levels: f32) -> f32 {
    if levels <= 1.0 {
        return v;
    }
    let step = 255.0 / (levels - 1.0);
    (v / step).round() * step
}

/// RGB -> HSV. H в 0..360, S и V в 0..1.
fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let v = max;
    let s = if max <= 0.0 { 0.0 } else { d / max };
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h.rem_euclid(360.0), s, v)
}

/// HSV -> RGB. H в 0..360, S и V в 0..1.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(360.0);
    let c = v * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (r1 + m, g1 + m, b1 + m)
}

/// Схемы перестановки каналов R/G/B.
const PERMS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [1, 0, 2],
    [2, 1, 0],
    [0, 2, 1],
    [1, 2, 0],
    [2, 0, 1],
];

/// Разводит красный и синий каналы по углу, с построчным дрожанием.
pub struct RgbSplit;

impl Effect for RgbSplit {
    fn id(&self) -> &'static str {
        "rgb_split"
    }

    fn name(&self) -> &'static str {
        "RGB Split"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Color
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("amount", "Смещение, px", 0.0, 100.0, 8.0, 1.0),
            ParamSpec::new("angle", "Угол, °", 0.0, 360.0, 0.0, 1.0),
            ParamSpec::new("jitter", "Дрожание строк", 0.0, 100.0, 20.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Разводит красный и синий каналы по углу; дрожание добавляет построчный сдвиг. \
         Смещение и дрожание зависят от Chaos; угол — фиксированный."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amount = sane(ctx.v(0), 0.0, 100.0, 8.0);
        let angle = sane(ctx.raw(1), 0.0, 360.0, 0.0);
        let jitter = sane(ctx.v(2), 0.0, 100.0, 20.0);
        if amount <= 0.0 && jitter <= 0.0 {
            return Ok(());
        }
        let rad = angle.to_radians();
        let dx = rad.cos() * amount;
        let dy = rad.sin() * amount;

        let mut jit: Vec<f32> = Vec::with_capacity(h);
        for _ in 0..h {
            jit.push(ctx.rng.range(-1.0, 1.0) * jitter);
        }

        let src = ctx.frame.clone();
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let jx = jit[y];
                let yf = y as f32;
                for x in 0..w {
                    let o = x * 4;
                    let xf = x as f32;
                    let r = src.sample_bilinear(xf + dx + jx, yf + dy)[0];
                    let g = src.sample_bilinear(xf, yf)[1];
                    let b = src.sample_bilinear(xf - dx - jx, yf - dy)[2];
                    row[o] = r.round().clamp(0.0, 255.0) as u8;
                    row[o + 1] = g.round().clamp(0.0, 255.0) as u8;
                    row[o + 2] = b.round().clamp(0.0, 255.0) as u8;
                    // альфа не трогается — остаётся исходной
                }
            });
        Ok(())
    }
}

/// Переставляет каналы R/G/B по одной из шести схем.
pub struct ChannelSwap;

impl Effect for ChannelSwap {
    fn id(&self) -> &'static str {
        "channel_swap"
    }

    fn name(&self) -> &'static str {
        "Channel Swap"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Color
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 2] = [
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 60.0, 1.0),
            ParamSpec::new("mode", "Схема", 0.0, 5.0, 1.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Переставляет каналы R/G/B по схеме 0..5. Сила зависит от Chaos; схема — фиксированная."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amount = sane(ctx.v(0), 0.0, 100.0, 60.0) / 100.0;
        if amount <= 0.0 {
            return Ok(());
        }
        let mode = sane(ctx.raw(1), 0.0, 5.0, 1.0).round() as usize;
        let perm = PERMS[mode.min(5)];
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride).for_each(|row| {
            for px in row.chunks_exact_mut(4) {
                let orig = [px[0], px[1], px[2]];
                for c in 0..3 {
                    let s = orig[perm[c]] as f32;
                    let d = orig[c] as f32;
                    px[c] = (d + (s - d) * amount).round().clamp(0.0, 255.0) as u8;
                }
            }
        });
        Ok(())
    }
}

/// Поворачивает тон и масштабирует насыщенность.
pub struct HueShift;

impl Effect for HueShift {
    fn id(&self) -> &'static str {
        "hue_shift"
    }

    fn name(&self) -> &'static str {
        "Hue Shift"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Color
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 2] = [
            ParamSpec::new("angle", "Сдвиг тона, °", 0.0, 360.0, 30.0, 1.0),
            ParamSpec::new("saturation", "Насыщенность, %", 0.0, 200.0, 100.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Поворачивает тон на угол и масштабирует насыщенность. \
         Угол зависит от Chaos; насыщенность — фиксированная."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let angle = sane(ctx.v(0), 0.0, 360.0, 30.0);
        let sat_scale = sane(ctx.raw(1), 0.0, 200.0, 100.0) / 100.0;
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride).for_each(|row| {
            for px in row.chunks_exact_mut(4) {
                let (hh, ss, vv) = rgb_to_hsv(px[0] as f32, px[1] as f32, px[2] as f32);
                let (r, g, b) = hsv_to_rgb(hh + angle, (ss * sat_scale).clamp(0.0, 1.0), vv);
                px[0] = r.round().clamp(0.0, 255.0) as u8;
                px[1] = g.round().clamp(0.0, 255.0) as u8;
                px[2] = b.round().clamp(0.0, 255.0) as u8;
            }
        });
        Ok(())
    }
}

/// Инвертирует каналы ярче порога.
pub struct Solarize;

impl Effect for Solarize {
    fn id(&self) -> &'static str {
        "solarize"
    }

    fn name(&self) -> &'static str {
        "Solarize"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Color
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 2] = [
            ParamSpec::new("threshold", "Порог", 0.0, 255.0, 128.0, 1.0),
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 100.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Инвертирует каналы ярче порога. Сила зависит от Chaos; порог — фиксированный."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let thr = sane(ctx.raw(0), 0.0, 255.0, 128.0);
        let amount = sane(ctx.v(1), 0.0, 100.0, 100.0) / 100.0;
        if amount <= 0.0 {
            return Ok(());
        }
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride).for_each(|row| {
            for px in row.chunks_exact_mut(4) {
                for px_c in px.iter_mut().take(3) {
                    let v = *px_c as f32;
                    if v >= thr {
                        let inv = 255.0 - v;
                        *px_c = (v + (inv - v) * amount).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        });
        Ok(())
    }
}

/// Инвертирует пиксели, у которых максимум канала не ниже порога.
pub struct Invert;

impl Effect for Invert {
    fn id(&self) -> &'static str {
        "invert"
    }

    fn name(&self) -> &'static str {
        "Invert"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Color
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 2] = [
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 100.0, 1.0),
            ParamSpec::new("threshold", "Порог яркости", 0.0, 255.0, 0.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Инвертирует пиксели, у которых максимум канала не ниже порога. \
         Сила зависит от Chaos; порог — фиксированный."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amount = sane(ctx.v(0), 0.0, 100.0, 100.0) / 100.0;
        if amount <= 0.0 {
            return Ok(());
        }
        let thr = sane(ctx.raw(1), 0.0, 255.0, 0.0);
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride).for_each(|row| {
            for px in row.chunks_exact_mut(4) {
                let mx = px[0].max(px[1]).max(px[2]) as f32;
                if mx >= thr {
                    for px_c in px.iter_mut().take(3) {
                        let v = *px_c as f32;
                        let inv = 255.0 - v;
                        *px_c = (v + (inv - v) * amount).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        });
        Ok(())
    }
}

/// Сводит каналы к заданному числу уровней.
pub struct Posterize;

impl Effect for Posterize {
    fn id(&self) -> &'static str {
        "posterize"
    }

    fn name(&self) -> &'static str {
        "Posterize"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Color
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 2] = [
            ParamSpec::new("levels", "Уровней на канал", 2.0, 16.0, 6.0, 1.0),
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 100.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Сводит каналы к заданному числу уровней. Сила зависит от Chaos; число уровней — фиксированное."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let levels = sane(ctx.raw(0), 2.0, 16.0, 6.0);
        let amount = sane(ctx.v(1), 0.0, 100.0, 100.0) / 100.0;
        if amount <= 0.0 {
            return Ok(());
        }
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride).for_each(|row| {
            for px in row.chunks_exact_mut(4) {
                for px_c in px.iter_mut().take(3) {
                    let v = *px_c as f32;
                    let q = quantize(v, levels);
                    *px_c = (v + (q - v) * amount).round().clamp(0.0, 255.0) as u8;
                }
            }
        });
        Ok(())
    }
}

/// Растекает яркие цвета по горизонтали (максимум в окне).
pub struct ColorBleed;

impl Effect for ColorBleed {
    fn id(&self) -> &'static str {
        "color_bleed"
    }

    fn name(&self) -> &'static str {
        "Color Bleed"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Color
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 2] = [
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 60.0, 1.0),
            ParamSpec::new("radius", "Радиус, px", 1.0, 24.0, 4.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Растекает яркие цвета по горизонтали (максимум в окне). \
         Сила зависит от Chaos; радиус — фиксированный."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amount = sane(ctx.v(0), 0.0, 100.0, 60.0) / 100.0;
        if amount <= 0.0 {
            return Ok(());
        }
        let radius = sane(ctx.raw(1), 1.0, 24.0, 4.0).round() as usize;
        let radius = radius.max(1);
        let src = ctx.frame.clone();
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let srow = src.row(y as u32);
                for x in 0..w {
                    let o = x * 4;
                    let lo = x.saturating_sub(radius);
                    let hi = (x + radius).min(w - 1);
                    for c in 0..3 {
                        let mut mx = 0u8;
                        for xx in lo..=hi {
                            let v = srow[xx * 4 + c];
                            if v > mx {
                                mx = v;
                            }
                        }
                        let v = srow[o + c] as f32;
                        row[o + c] = (v + (mx as f32 - v) * amount).round().clamp(0.0, 255.0) as u8;
                    }
                }
            });
        Ok(())
    }
}

pub fn effects() -> Vec<Box<dyn Effect>> {
    vec![
        Box::new(RgbSplit),
        Box::new(ChannelSwap),
        Box::new(HueShift),
        Box::new(Solarize),
        Box::new(Invert),
        Box::new(Posterize),
        Box::new(ColorBleed),
    ]
}
