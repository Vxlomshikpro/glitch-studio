//! Группа «Геометрия»: сдвиг срезов, смещение блоков, волна, пикселизация,
//! дрожание строк и ошибка трекинга.

use crate::core::effect::{Effect, EffectCtx, EffectGroup};
use crate::core::error::GlitchError;
use crate::core::frame::Frame;
use crate::core::params::ParamSpec;
use rayon::prelude::*;

/// Безопасное значение ползунка: NaN/inf → fallback, иначе зажим в `[lo, hi]`.
#[inline]
fn sane(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

/// Выборка пикселя с заворотом координат по обеим осям.
#[inline]
fn sample_wrapped(src: &Frame, x: i64, y: i64) -> [u8; 4] {
    let w = src.width as i64;
    let h = src.height as i64;
    if w <= 0 || h <= 0 {
        return [0, 0, 0, 0];
    }
    let xx = x.rem_euclid(w);
    let yy = y.rem_euclid(h);
    src.get(xx, yy).unwrap_or([0, 0, 0, 0])
}

/// Сдвигает горизонтальные срезы вбок на случайную величину.
pub struct SliceShift;

impl Effect for SliceShift {
    fn id(&self) -> &'static str {
        "slice_shift"
    }

    fn name(&self) -> &'static str {
        "Slice Shift"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Geometry
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("amount", "Смещение, px", 0.0, 100.0, 45.0, 1.0),
            ParamSpec::new("density", "Плотность, %", 0.0, 100.0, 30.0, 1.0),
            ParamSpec::new("slice_height", "Высота среза, px", 1.0, 64.0, 8.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Сдвигает горизонтальные срезы вбок. Смещение зависит от Chaos; \
         плотность и высота среза — фиксированные."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amount = sane(ctx.v(0), 0.0, 100.0, 45.0);
        let density = sane(ctx.raw(1), 0.0, 100.0, 30.0) / 100.0;
        let sh = sane(ctx.raw(2), 1.0, 64.0, 8.0).round() as usize;
        let sh = sh.max(1);
        if amount <= 0.0 || density <= 0.0 {
            return Ok(());
        }
        let n_slices = h.div_ceil(sh);
        let mut shifts: Vec<i64> = Vec::with_capacity(n_slices);
        for _ in 0..n_slices {
            if ctx.rng.chance(density) {
                let mut s = ctx.rng.range(-amount, amount).round() as i64;
                if s == 0 {
                    s = 1;
                }
                shifts.push(s);
            } else {
                shifts.push(0);
            }
        }
        let src = ctx.frame.clone();
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let s = shifts[y / sh];
                if s == 0 {
                    return;
                }
                for x in 0..w {
                    let o = x * 4;
                    let px = sample_wrapped(&src, x as i64 - s, y as i64);
                    row[o..o + 4].copy_from_slice(&px);
                }
            });
        Ok(())
    }
}

/// Сдвигает квадратные блоки в случайные стороны.
pub struct BlockDisplacement;

impl Effect for BlockDisplacement {
    fn id(&self) -> &'static str {
        "block_displacement"
    }

    fn name(&self) -> &'static str {
        "Block Displacement"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Geometry
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 2] = [
            ParamSpec::new("amount", "Смещение, px", 0.0, 100.0, 60.0, 1.0),
            ParamSpec::new("block_size", "Размер блока, px", 2.0, 64.0, 16.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Сдвигает квадратные блоки в случайные стороны. \
         Смещение зависит от Chaos; размер блока — фиксированный."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amount = sane(ctx.v(0), 0.0, 100.0, 60.0);
        let bs = sane(ctx.raw(1), 2.0, 64.0, 16.0).round() as usize;
        let bs = bs.max(1);
        if amount <= 0.0 {
            return Ok(());
        }
        let bw = w.div_ceil(bs);
        let bh = h.div_ceil(bs);
        let mut offs: Vec<(i64, i64)> = Vec::with_capacity(bw * bh);
        for _ in 0..(bw * bh) {
            let mut dx = ctx.rng.range(-amount, amount).round() as i64;
            let mut dy = ctx.rng.range(-amount, amount).round() as i64;
            if dx == 0 {
                dx = 1;
            }
            if dy == 0 {
                dy = 1;
            }
            offs.push((dx, dy));
        }
        let src = ctx.frame.clone();
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let by = y / bs;
                for x in 0..w {
                    let bx = x / bs;
                    let (dx, dy) = offs[by * bw + bx];
                    let o = x * 4;
                    let px = sample_wrapped(&src, x as i64 - dx, y as i64 - dy);
                    row[o..o + 4].copy_from_slice(&px);
                }
            });
        Ok(())
    }
}

/// Синусоидально смещает строки или столбцы.
pub struct Wave;

impl Effect for Wave {
    fn id(&self) -> &'static str {
        "wave"
    }

    fn name(&self) -> &'static str {
        "Wave"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Geometry
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("amplitude", "Амплитуда, px", 0.0, 100.0, 5.0, 1.0),
            ParamSpec::new("wavelength", "Длина волны, px", 2.0, 200.0, 40.0, 1.0),
            ParamSpec::new("direction", "Направление", 0.0, 1.0, 0.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Синусоидально смещает строки (0) или столбцы (1). \
         Амплитуда зависит от Chaos; длина волны и направление — фиксированные."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amp = sane(ctx.v(0), 0.0, 100.0, 5.0);
        let wl = sane(ctx.raw(1), 2.0, 200.0, 40.0);
        let dir = sane(ctx.raw(2), 0.0, 1.0, 0.0).round() as u8;
        if amp <= 0.0 {
            return Ok(());
        }
        let tau = std::f32::consts::TAU;
        let src = ctx.frame.clone();
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        if dir == 0 {
            data.par_chunks_mut(stride)
                .enumerate()
                .for_each(|(y, row)| {
                    let off = (amp * (tau * y as f32 / wl).sin()).round() as i64;
                    for x in 0..w {
                        let o = x * 4;
                        let px = sample_wrapped(&src, x as i64 - off, y as i64);
                        row[o..o + 4].copy_from_slice(&px);
                    }
                });
        } else {
            data.par_chunks_mut(stride)
                .enumerate()
                .for_each(|(y, row)| {
                    for x in 0..w {
                        let off = (amp * (tau * x as f32 / wl).sin()).round() as i64;
                        let o = x * 4;
                        let px = sample_wrapped(&src, x as i64, y as i64 - off);
                        row[o..o + 4].copy_from_slice(&px);
                    }
                });
        }
        Ok(())
    }
}

/// Усредняет блоки пикселей.
pub struct Pixelate;

impl Effect for Pixelate {
    fn id(&self) -> &'static str {
        "pixelate"
    }

    fn name(&self) -> &'static str {
        "Pixelate"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Geometry
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 2] = [
            ParamSpec::new("size", "Размер блока, px", 1.0, 64.0, 7.0, 1.0),
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 100.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Усредняет блоки пикселей. Сила зависит от Chaos; размер блока — фиксированный."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let size_raw = sane(ctx.raw(0), 1.0, 64.0, 7.0);
        // Именно `!(x > 0.0)`, а не `x <= 0.0`: при NaN второй вариант даёт
        // false и мусор проходит дальше, первый — true и эффект выключается.
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(size_raw > 0.0) {
            return Ok(());
        }
        let size = (size_raw.round() as usize).max(1);
        let amount = sane(ctx.v(1), 0.0, 100.0, 100.0) / 100.0;
        if amount <= 0.0 {
            return Ok(());
        }
        let src = ctx.frame.clone();
        let bw = w.div_ceil(size);
        let bh = h.div_ceil(size);
        let mut avgs: Vec<[f32; 3]> = vec![[0.0; 3]; bw * bh];
        for by in 0..bh {
            let y0 = by * size;
            let y1 = (y0 + size).min(h);
            for bx in 0..bw {
                let x0 = bx * size;
                let x1 = (x0 + size).min(w);
                let mut acc = [0u32; 3];
                let mut cnt = 0u32;
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        if let Some(p) = src.get(xx as i64, yy as i64) {
                            acc[0] += p[0] as u32;
                            acc[1] += p[1] as u32;
                            acc[2] += p[2] as u32;
                            cnt += 1;
                        }
                    }
                }
                if cnt > 0 {
                    avgs[by * bw + bx] = [
                        acc[0] as f32 / cnt as f32,
                        acc[1] as f32 / cnt as f32,
                        acc[2] as f32 / cnt as f32,
                    ];
                }
            }
        }
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let by = y / size;
                for x in 0..w {
                    let bx = x / size;
                    let avg = avgs[by * bw + bx];
                    let o = x * 4;
                    for c in 0..3 {
                        let v = row[o + c] as f32;
                        row[o + c] = (v + (avg[c] - v) * amount).round().clamp(0.0, 255.0) as u8;
                    }
                }
            });
        Ok(())
    }
}

/// Дрожит строки по горизонтали и яркости.
pub struct ScanJitter;

impl Effect for ScanJitter {
    fn id(&self) -> &'static str {
        "scan_jitter"
    }

    fn name(&self) -> &'static str {
        "Scan Jitter"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Geometry
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("amount", "Смещение, px", 0.0, 100.0, 40.0, 1.0),
            ParamSpec::new("density", "Плотность, %", 0.0, 100.0, 50.0, 1.0),
            ParamSpec::new("brightness", "Дрожание яркости, %", 0.0, 100.0, 0.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Дрожит строки по горизонтали и яркости. Смещение и яркость зависят от Chaos; \
         плотность — фиксированная."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amount = sane(ctx.v(0), 0.0, 100.0, 40.0);
        let density = sane(ctx.raw(1), 0.0, 100.0, 50.0) / 100.0;
        let bright = sane(ctx.v(2), 0.0, 100.0, 0.0) / 100.0;
        if amount <= 0.0 && bright <= 0.0 {
            return Ok(());
        }
        let mut plan: Vec<(i64, f32)> = Vec::with_capacity(h);
        for _ in 0..h {
            if ctx.rng.chance(density) {
                let mut off = ctx.rng.range(-amount, amount).round() as i64;
                if off == 0 {
                    off = 1;
                }
                let bf = 1.0 + ctx.rng.range(-1.0, 1.0) * bright;
                plan.push((off, bf));
            } else {
                plan.push((0, 1.0));
            }
        }
        let src = ctx.frame.clone();
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let (off, bf) = plan[y];
                for x in 0..w {
                    let o = x * 4;
                    let px = sample_wrapped(&src, x as i64 - off, y as i64);
                    for c in 0..3 {
                        row[o + c] = (px[c] as f32 * bf).round().clamp(0.0, 255.0) as u8;
                    }
                    row[o + 3] = px[3];
                }
            });
        Ok(())
    }
}

/// Рвёт картинку случайными горизонтальными полосами со сдвигом.
pub struct TrackingError;

impl Effect for TrackingError {
    fn id(&self) -> &'static str {
        "tracking_error"
    }

    fn name(&self) -> &'static str {
        "Tracking Error"
    }

    fn group(&self) -> EffectGroup {
        EffectGroup::Geometry
    }

    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("amount", "Смещение, px", 0.0, 100.0, 30.0, 1.0),
            ParamSpec::new("height", "Высота полосы, px", 1.0, 64.0, 12.0, 1.0),
            ParamSpec::new("count", "Число полос", 1.0, 8.0, 2.0, 1.0),
        ];
        &P
    }

    fn hint(&self) -> &'static str {
        "Рвёт картинку случайными горизонтальными полосами со сдвигом. \
         Смещение зависит от Chaos; высота и число полос — фиксированные."
    }

    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let w = ctx.frame.width as usize;
        let h = ctx.frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }
        let amount = sane(ctx.v(0), 0.0, 100.0, 30.0);
        let bh = sane(ctx.raw(1), 1.0, 64.0, 12.0).round() as usize;
        let bh = bh.max(1);
        let count = sane(ctx.raw(2), 1.0, 8.0, 2.0).round() as usize;
        let count = count.max(1);
        if amount <= 0.0 {
            return Ok(());
        }
        let mut shifts: Vec<i64> = vec![0; h];
        for _ in 0..count {
            let start = ctx.rng.range_i(0, h as i64 - 1) as usize;
            let mut off = ctx.rng.range(-amount, amount).round() as i64;
            if off == 0 {
                off = 1;
            }
            let end = (start + bh).min(h);
            for s in shifts.iter_mut().take(end).skip(start) {
                *s = off;
            }
        }
        let src = ctx.frame.clone();
        let stride = w * 4;
        let data = &mut ctx.frame.data;
        data.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let off = shifts[y];
                if off == 0 {
                    return;
                }
                for x in 0..w {
                    let o = x * 4;
                    let px = sample_wrapped(&src, x as i64 - off, y as i64);
                    row[o..o + 4].copy_from_slice(&px);
                }
            });
        Ok(())
    }
}

pub fn effects() -> Vec<Box<dyn Effect>> {
    vec![
        Box::new(SliceShift),
        Box::new(BlockDisplacement),
        Box::new(Wave),
        Box::new(Pixelate),
        Box::new(ScanJitter),
        Box::new(TrackingError),
    ]
}
