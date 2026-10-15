//! Временные (видео) эффекты: работают с историей предыдущих кадров.
//!
//! Для одиночной картинки `prev` пуст — каждый эффект либо тихо ничего не делает,
//! либо деградирует до «статичной» версии (ghosting рисует синтетический размаз).

use rayon::prelude::*;

use crate::core::effect::{Effect, EffectCtx, EffectGroup};
use crate::core::error::GlitchError;
use crate::core::frame::Frame;
use crate::core::params::ParamSpec;

/// Кадр `back` шагов назад. Безопасно при пустой истории и любом `back`.
#[inline]
fn past_frame(prev: &[std::sync::Arc<Frame>], back: usize) -> Option<&Frame> {
    prev.len()
        .checked_sub(back + 1)
        .and_then(|i| prev.get(i))
        .map(|f| f.as_ref())
}

/// Цвет пикселя с поканальным сдвигом оттенка призрака.
#[inline]
fn tint_px(px: [u8; 4], tint: f32) -> [f32; 3] {
    [
        px[0] as f32 * (1.0 + tint * 0.6),
        px[1] as f32,
        px[2] as f32 * (1.0 - tint * 0.6),
    ]
}

/// Дешёвый детерминированный шум 0..1 по координате и seed.
#[inline]
fn hash_noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(y.wrapping_mul(0x85EB_CA6B))
        ^ seed;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

// ---------------------------------------------------------------------------
// 1. Ghosting
// ---------------------------------------------------------------------------

pub struct Ghosting;

impl Effect for Ghosting {
    fn id(&self) -> &'static str {
        "ghosting"
    }
    fn name(&self) -> &'static str {
        "Ghosting"
    }
    fn group(&self) -> EffectGroup {
        EffectGroup::Temporal
    }
    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 4] = [
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 50.0, 1.0),
            ParamSpec::new("decay", "Затухание", 0.0, 100.0, 60.0, 1.0),
            ParamSpec::new("history", "История, кадров", 1.0, 8.0, 3.0, 1.0),
            ParamSpec::new("tint", "Оттенок", 0.0, 100.0, 0.0, 1.0),
        ];
        &P
    }
    fn hint(&self) -> &'static str {
        "Остаточный след от предыдущих кадров. На фото работает как размаз."
    }
    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let amount = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        if amount <= 0.0 {
            return Ok(());
        }
        let decay = (ctx.v(1) / 100.0).clamp(0.0, 1.0);
        let history = (ctx.raw(2).round() as i64).clamp(1, 8) as usize;
        let tint = (ctx.v(3) / 100.0).clamp(0.0, 1.0);

        let prev = ctx.prev;
        let frame = &mut *ctx.frame;
        let w = frame.width as usize;
        let h = frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }

        let mut ghosts: Vec<&Frame> = Vec::new();
        for k in 0..history {
            if let Some(p) = past_frame(prev, k) {
                if p.width == frame.width && p.height == frame.height {
                    ghosts.push(p);
                }
            }
        }

        let stride = w * 4;

        if ghosts.is_empty() {
            // Нет истории — синтетический размаз: смешиваем кадр с собой,
            // сдвинутым по горизонтали, чтобы эффект был виден на фото.
            let snapshot = (*frame).clone();
            let off = ((amount * w as f32 * 0.03).round() as i64).max(1);
            let weight = amount * 0.6;
            frame
                .data
                .par_chunks_mut(stride)
                .enumerate()
                .for_each(|(y, row)| {
                    for (x, px) in row.chunks_exact_mut(4).enumerate() {
                        if let Some(g) = snapshot.get(x as i64 - off, y as i64) {
                            let gt = tint_px(g, tint);
                            for c in 0..3 {
                                let cur = px[c] as f32;
                                px[c] = (cur * (1.0 - weight) + gt[c] * weight)
                                    .round()
                                    .clamp(0.0, 255.0)
                                    as u8;
                            }
                        }
                    }
                });
            return Ok(());
        }

        frame
            .data
            .par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                for (x, px) in row.chunks_exact_mut(4).enumerate() {
                    let mut col = [px[0] as f32, px[1] as f32, px[2] as f32];
                    for (k, g) in ghosts.iter().enumerate() {
                        let wgt = amount * decay.powi(k as i32);
                        if wgt <= 0.0 {
                            continue;
                        }
                        if let Some(gp) = g.get(x as i64, y as i64) {
                            let gt = tint_px(gp, tint);
                            for c in 0..3 {
                                col[c] = col[c] * (1.0 - wgt) + gt[c] * wgt;
                            }
                        }
                    }
                    for c in 0..3 {
                        px[c] = col[c].round().clamp(0.0, 255.0) as u8;
                    }
                }
            });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 2. Feedback
// ---------------------------------------------------------------------------

pub struct Feedback;

impl Effect for Feedback {
    fn id(&self) -> &'static str {
        "feedback"
    }
    fn name(&self) -> &'static str {
        "Feedback"
    }
    fn group(&self) -> EffectGroup {
        EffectGroup::Temporal
    }
    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 4] = [
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 40.0, 1.0),
            ParamSpec::new("zoom", "Масштаб, %", 95.0, 105.0, 99.0, 1.0),
            ParamSpec::new("rotate", "Поворот, °", 0.0, 360.0, 0.0, 1.0),
            ParamSpec::new("decay", "Затухание", 0.0, 100.0, 70.0, 1.0),
        ];
        &P
    }
    fn hint(&self) -> &'static str {
        "Обратная связь: прошлый кадр с зумом и поворотом аддитивно ложится на текущий."
    }
    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let amount = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        if amount <= 0.0 {
            return Ok(());
        }
        let zoom = (ctx.raw(1) / 100.0).clamp(0.1, 10.0);
        let rotate = ctx.raw(2);
        let decay = (ctx.v(3) / 100.0).clamp(0.0, 1.0);
        let strength = amount * decay;
        if strength <= 0.0 {
            return Ok(());
        }

        let prev = ctx.prev;
        let frame = &mut *ctx.frame;
        let w = frame.width as usize;
        let h = frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }

        let Some(past) = past_frame(prev, 0) else {
            return Ok(());
        };
        if past.width != frame.width || past.height != frame.height {
            return Ok(());
        }

        let angle = rotate.to_radians();
        let (sin, cos) = angle.sin_cos();
        let inv_zoom = 1.0 / zoom;
        let cx = (w as f32 - 1.0) * 0.5;
        let cy = (h as f32 - 1.0) * 0.5;
        let max_x = (w as f32 - 1.0).max(0.0);
        let max_y = (h as f32 - 1.0).max(0.0);
        let stride = w * 4;

        frame
            .data
            .par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let dy = y as f32 - cy;
                for (x, px) in row.chunks_exact_mut(4).enumerate() {
                    let dx = x as f32 - cx;
                    // Обратное преобразование: поворот на -angle, затем деление на zoom.
                    let rx = dx * cos + dy * sin;
                    let ry = -dx * sin + dy * cos;
                    let sx = (cx + rx * inv_zoom).clamp(0.0, max_x).round() as i64;
                    let sy = (cy + ry * inv_zoom).clamp(0.0, max_y).round() as i64;
                    if let Some(g) = past.get(sx, sy) {
                        for c in 0..3 {
                            let v = px[c] as f32 + g[c] as f32 * strength;
                            px[c] = v.clamp(0.0, 255.0) as u8;
                        }
                    }
                }
            });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 3. Frame Stutter
// ---------------------------------------------------------------------------

pub struct FrameStutter;

impl Effect for FrameStutter {
    fn id(&self) -> &'static str {
        "frame_stutter"
    }
    fn name(&self) -> &'static str {
        "Frame Stutter"
    }
    fn group(&self) -> EffectGroup {
        EffectGroup::Temporal
    }
    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("hold", "Удержание", 0.0, 100.0, 30.0, 1.0),
            ParamSpec::new("freeze", "Глубина, кадров", 1.0, 12.0, 3.0, 1.0),
            ParamSpec::new("glitch", "Порча", 0.0, 100.0, 40.0, 1.0),
        ];
        &P
    }
    fn hint(&self) -> &'static str {
        "Стоп-кадр: вместо текущего кадра подставляется испорченный старый."
    }
    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let hold = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        if hold <= 0.0 {
            return Ok(());
        }
        let freeze = (ctx.raw(1).round() as i64).clamp(1, 12) as usize;
        let glitch = (ctx.v(2) / 100.0).clamp(0.0, 1.0);

        let prev = ctx.prev;
        let rng = &mut *ctx.rng;
        let frame = &mut *ctx.frame;
        let w = frame.width as usize;
        let h = frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }

        if !rng.chance(hold) {
            return Ok(());
        }

        let back = freeze.saturating_sub(1);
        let held = past_frame(prev, back).or_else(|| prev.last().map(|f| f.as_ref()));
        let Some(held) = held else {
            return Ok(());
        };
        if held.width != frame.width || held.height != frame.height {
            return Ok(());
        }

        frame.copy_from(held);

        // Параметры порчи по строкам генерируем заранее, чтобы потом
        // распараллелить построчную обработку без общего Rng.
        let mut plan: Vec<(i64, [f32; 3], u32)> = Vec::with_capacity(h);
        for _ in 0..h {
            if rng.chance(glitch) {
                let shift = rng.range_i(-(w as i64), w as i64);
                let bias = [
                    rng.range(-glitch, glitch) * 80.0,
                    rng.range(-glitch, glitch) * 80.0,
                    rng.range(-glitch, glitch) * 80.0,
                ];
                let seed = rng.below(u32::MAX);
                plan.push((shift, bias, seed));
            } else {
                plan.push((0, [0.0; 3], 0));
            }
        }

        let stride = w * 4;
        frame
            .data
            .par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let Some(&(shift, bias, seed)) = plan.get(y) else {
                    return;
                };
                for (x, px) in row.chunks_exact_mut(4).enumerate() {
                    let sx = (x as i64 - shift).rem_euclid(w as i64);
                    let mut col = match held.get(sx, y as i64) {
                        Some(g) => [g[0] as f32, g[1] as f32, g[2] as f32],
                        None => [px[0] as f32, px[1] as f32, px[2] as f32],
                    };
                    if seed != 0 {
                        let n = (hash_noise(x as u32, y as u32, seed) - 0.5) * glitch * 120.0;
                        for c in 0..3 {
                            col[c] += n + bias[c];
                        }
                    }
                    for c in 0..3 {
                        px[c] = col[c].clamp(0.0, 255.0) as u8;
                    }
                }
            });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 4. Frame Drop
// ---------------------------------------------------------------------------

pub struct FrameDrop;

impl Effect for FrameDrop {
    fn id(&self) -> &'static str {
        "frame_drop"
    }
    fn name(&self) -> &'static str {
        "Frame Drop"
    }
    fn group(&self) -> EffectGroup {
        EffectGroup::Temporal
    }
    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 3] = [
            ParamSpec::new("amount", "Доля кадра, %", 0.0, 100.0, 25.0, 1.0),
            ParamSpec::new("rows", "Доля строк, %", 0.0, 100.0, 60.0, 1.0),
            ParamSpec::new("jitter", "Дрожание", 0.0, 100.0, 50.0, 1.0),
        ];
        &P
    }
    fn hint(&self) -> &'static str {
        "Частичные выпадения: случайные полосы строк откатываются к прошлому кадру."
    }
    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let amount = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        if amount <= 0.0 {
            return Ok(());
        }
        let rows_pct = (ctx.v(1) / 100.0).clamp(0.0, 1.0);
        let jitter = (ctx.v(2) / 100.0).clamp(0.0, 1.0);

        let prev = ctx.prev;
        let rng = &mut *ctx.rng;
        let frame = &mut *ctx.frame;
        let w = frame.width as usize;
        let h = frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }

        let Some(past) = past_frame(prev, 0) else {
            return Ok(());
        };
        if past.width != frame.width || past.height != frame.height {
            return Ok(());
        }

        let band = ((amount * w as f32).round() as i64).clamp(0, w as i64);
        if band == 0 {
            return Ok(());
        }

        let max_start = (w as i64 - band).max(0);
        let center = max_start / 2;
        let span = (max_start as f32 * jitter).round() as i64;

        let mut plan: Vec<Option<(i64, i64)>> = Vec::with_capacity(h);
        for _ in 0..h {
            if rng.chance(rows_pct) {
                let off = if span > 0 {
                    rng.range_i(-span, span)
                } else {
                    0
                };
                let start = (center + off).clamp(0, max_start);
                plan.push(Some((start, band)));
            } else {
                plan.push(None);
            }
        }

        let stride = w * 4;
        frame
            .data
            .par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let Some((start, len)) = plan.get(y).copied().flatten() else {
                    return;
                };
                let start_u = start.max(0) as usize;
                let len_u = len.max(0) as usize;
                for (i, px) in row
                    .chunks_exact_mut(4)
                    .skip(start_u)
                    .take(len_u)
                    .enumerate()
                {
                    let x = start_u + i;
                    if let Some(g) = past.get(x as i64, y as i64) {
                        px[0] = g[0];
                        px[1] = g[1];
                        px[2] = g[2];
                    }
                }
            });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 5. Temporal Jitter
// ---------------------------------------------------------------------------

struct JitterBlock {
    disp: i64,
    blend_prev: bool,
}

pub struct TemporalJitter;

impl Effect for TemporalJitter {
    fn id(&self) -> &'static str {
        "temporal_jitter"
    }
    fn name(&self) -> &'static str {
        "Temporal Jitter"
    }
    fn group(&self) -> EffectGroup {
        EffectGroup::Temporal
    }
    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 4] = [
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 40.0, 1.0),
            ParamSpec::new("jitter_x", "Сдвиг по X", 0.0, 100.0, 100.0, 1.0),
            ParamSpec::new("blocks", "Блоков", 1.0, 16.0, 4.0, 1.0),
            ParamSpec::new("hold", "Удержание", 0.0, 100.0, 20.0, 1.0),
        ];
        &P
    }
    fn hint(&self) -> &'static str {
        "Блочное дрожание по горизонтали, зависящее от движения между кадрами."
    }
    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let amount = (ctx.v(0) / 100.0).clamp(0.0, 1.0);
        if amount <= 0.0 {
            return Ok(());
        }
        let jitter_x = (ctx.v(1) / 100.0).clamp(0.0, 1.0);
        let blocks = (ctx.raw(2).round() as i64).clamp(1, 16) as usize;
        let hold = (ctx.v(3) / 100.0).clamp(0.0, 1.0);

        let prev = ctx.prev;
        let rng = &mut *ctx.rng;
        let frame = &mut *ctx.frame;
        let w = frame.width as usize;
        let h = frame.height as usize;
        if w == 0 || h == 0 {
            return Ok(());
        }

        let past =
            past_frame(prev, 0).filter(|p| p.width == frame.width && p.height == frame.height);

        let block_h = h.div_ceil(blocks);
        let mut plan: Vec<JitterBlock> = Vec::with_capacity(blocks);
        for b in 0..blocks {
            let y0 = b * block_h;
            let y1 = ((b + 1) * block_h).min(h);
            if y0 >= y1 {
                plan.push(JitterBlock {
                    disp: 0,
                    blend_prev: false,
                });
                continue;
            }
            // Метрика движения: средняя абсолютная разница с прошлым кадром.
            let motion = match past {
                Some(p) => {
                    let step_y = ((y1 - y0) / 8).max(1);
                    let step_x = (w / 16).max(1);
                    let mut sum = 0.0f32;
                    let mut cnt = 0.0f32;
                    let mut yy = y0;
                    while yy < y1 {
                        let mut xx = 0;
                        while xx < w {
                            if let (Some(a), Some(c)) =
                                (frame.get(xx as i64, yy as i64), p.get(xx as i64, yy as i64))
                            {
                                sum += (a[0] as f32 - c[0] as f32).abs()
                                    + (a[1] as f32 - c[1] as f32).abs()
                                    + (a[2] as f32 - c[2] as f32).abs();
                                cnt += 3.0;
                            }
                            xx += step_x;
                        }
                        yy += step_y;
                    }
                    if cnt > 0.0 {
                        (sum / cnt / 255.0).clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                }
                // Пустая история — двигаем блоки случайно.
                None => rng.unit(),
            };
            let sign = if rng.chance(0.5) { 1.0 } else { -1.0 };
            let disp = (amount * jitter_x * motion * w as f32 * 0.5 * sign).round() as i64;
            let blend_prev = past.is_some() && rng.chance(hold);
            plan.push(JitterBlock { disp, blend_prev });
        }

        let stride = w * 4;
        frame
            .data
            .par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| {
                let b = (y / block_h).min(blocks - 1);
                let Some(bp) = plan.get(b) else {
                    return;
                };
                if bp.blend_prev {
                    if let Some(p) = past {
                        for (x, px) in row.chunks_exact_mut(4).enumerate() {
                            if let Some(g) = p.get(x as i64, y as i64) {
                                for c in 0..3 {
                                    let cur = px[c] as f32;
                                    px[c] = (cur * (1.0 - amount) + g[c] as f32 * amount)
                                        .round()
                                        .clamp(0.0, 255.0)
                                        as u8;
                                }
                            }
                        }
                    }
                } else if bp.disp != 0 {
                    let src = row.to_vec();
                    for (x, px) in row.chunks_exact_mut(4).enumerate() {
                        let sx = (x as i64 - bp.disp).rem_euclid(w as i64) as usize;
                        let o = sx * 4;
                        if o + 3 < src.len() {
                            px[0] = src[o];
                            px[1] = src[o + 1];
                            px[2] = src[o + 2];
                        }
                    }
                }
            });
        Ok(())
    }
}

// ---------------------------------------------------------------------------

pub fn effects() -> Vec<Box<dyn Effect>> {
    vec![
        Box::new(Ghosting),
        Box::new(Feedback),
        Box::new(FrameStutter),
        Box::new(FrameDrop),
        Box::new(TemporalJitter),
    ]
}
