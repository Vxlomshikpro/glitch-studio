//! Темы Monet + glitchcore-стилистика.
//!
//! This module is the single source of truth for color. [`visuals`] and
//! [`style`] turn a [`Palette`] into egui's `Visuals`/`Style`; widgets must
//! never hardcode a color. The glitch painters ([`glitch_title`],
//! [`scanline_overlay`], [`glitch_offset`]) are deterministic given their
//! inputs, so animation is fully controlled by the caller.

mod builtins;
mod dynamic;
mod palette;

pub use builtins::{builtin_themes, theme_names};
pub use dynamic::{
    extract_from_image, extract_from_pixels, extract_from_wallpaper, DynamicPalette,
};
pub use palette::Palette;

use std::collections::BTreeMap;

use egui::style::{HandleShape, ScrollStyle, WidgetVisuals};
use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Vec2};

/// Builds the full egui `Visuals` from a [`Palette`].
///
/// This is the ONLY place where colors are set — widgets must never hardcode a
/// color.
pub fn visuals(palette: &Palette) -> egui::Visuals {
    let mut v = if palette.is_light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    v.dark_mode = !palette.is_light;

    v.panel_fill = palette.background;
    v.window_fill = palette.surface_container;
    v.window_stroke = Stroke::new(1.0, palette.outline);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.faint_bg_color = palette.surface_variant;
    v.extreme_bg_color = palette.surface_container_low;
    v.code_bg_color = palette.surface_container_high;
    v.text_edit_bg_color = Some(palette.surface_container_low);
    v.hyperlink_color = palette.primary;
    v.warn_fg_color = palette.tertiary;
    v.error_fg_color = palette.error;
    v.override_text_color = Some(palette.on_surface);
    v.weak_text_color = Some(palette.on_surface_variant);

    v.selection.bg_fill = palette.primary.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, palette.on_primary);

    // Slider: thin rail (via `Spacing::slider_rail_height`), bright trailing
    // fill (via `selection.bg_fill`), narrow rectangular handle.
    v.slider_trailing_fill = true;
    v.handle_shape = HandleShape::Rect { aspect_ratio: 0.45 };

    let cr = CornerRadius::same(6);
    let mut w = v.widgets;
    w.noninteractive = widget(
        palette.surface_container,
        palette.on_surface,
        palette.outline,
        cr,
    );
    w.inactive = widget(
        palette.surface_container_high,
        palette.on_surface,
        palette.outline,
        cr,
    );
    w.hovered = widget(
        palette.surface_container_high,
        palette.primary,
        palette.primary,
        cr,
    );
    w.active = widget(
        palette.primary_container,
        palette.on_primary_container,
        palette.primary,
        cr,
    );
    w.open = widget(
        palette.surface_container_high,
        palette.on_surface,
        palette.outline,
        cr,
    );
    v.widgets = w;
    v
}

/// Builds the full egui `Style` (visuals + spacing + text styles) from a
/// [`Palette`].
pub fn style(palette: &Palette) -> egui::Style {
    let mut s = egui::Style {
        visuals: visuals(palette),
        ..Default::default()
    };
    s.spacing.item_spacing = egui::vec2(8.0, 6.0);
    s.spacing.button_padding = egui::vec2(8.0, 4.0);
    s.spacing.window_margin = egui::Margin::same(10);
    s.spacing.menu_margin = egui::Margin::same(8);
    s.spacing.interact_size = egui::vec2(48.0, 22.0);
    s.spacing.slider_width = 120.0;
    s.spacing.slider_rail_height = 4.0;
    s.spacing.indent = 16.0;
    s.spacing.scroll = ScrollStyle::thin();
    s.animation_time = if palette.is_light { 0.15 } else { 0.2 };
    s.text_styles = text_styles();
    s
}

/// Applies a palette to a `Context` in one call.
///
/// Also stashes the palette in egui memory so the glitch painters can read the
/// channel colors without the caller threading them through.
pub fn apply(ctx: &egui::Context, palette: &Palette) {
    let theme = if palette.is_light {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    };
    ctx.set_theme(if palette.is_light {
        egui::ThemePreference::Light
    } else {
        egui::ThemePreference::Dark
    });
    ctx.set_style_of(theme, style(palette));
    ctx.data_mut(|d| d.insert_temp(palette_id(), palette.clone()));
}

/// RGB-split title: draws the same text three times offset by a few px in
/// primary/secondary/tertiary, with an occasional displaced dropout band.
/// Returns the text rect.
pub fn glitch_title(ui: &mut egui::Ui, text: &str, glitch_amount: f32) -> egui::Rect {
    let pal = current_palette(ui.ctx());
    let font = TextStyle::Heading.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let size = galley.size();
    let amount = glitch_amount.clamp(0.0, 12.0);
    let pad = egui::vec2(amount * 2.0, amount);
    let (rect, _resp) = ui.allocate_exact_size(size + pad, egui::Sense::hover());
    let base = rect.min + egui::vec2(amount, amount * 0.5);
    let painter = ui.painter();

    if amount <= 0.01 {
        painter.galley(base, galley, pal.on_surface);
        return rect;
    }

    let pass = ui.ctx().cumulative_pass_nr();
    let seed = hash_str(text);
    let r_off = glitch_offset(seed ^ 0x11, amount);
    let g_off = glitch_offset(seed ^ 0x22, amount);
    let b_off = glitch_offset(seed ^ 0x33, amount);

    // Channel split.
    painter.galley(base + r_off, galley.clone(), pal.primary);
    painter.galley(base + g_off, galley.clone(), pal.secondary);
    painter.galley(base + b_off, galley.clone(), pal.tertiary);
    // Core pass keeps the text readable.
    painter.galley(base, galley.clone(), pal.on_surface);

    // Occasional dropout: a thin band is displaced sideways.
    if amount > 1.0 && (pass.wrapping_mul(0x9E37_79B9) ^ seed) % 9 == 0 {
        let band = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.center().y - 2.0),
            egui::pos2(rect.right(), rect.center().y + 2.0),
        );
        let p = painter.with_clip_rect(band);
        let dx = glitch_offset(seed ^ 0x44, amount * 1.5).x;
        p.galley(base + egui::vec2(dx, 0.0), galley, pal.primary);
    }
    rect
}

/// Scanline + faint noise overlay over a rect. `strength` 0..1, 0 = off.
/// `phase` scrolls the scanlines (pass e.g. `time * 20.0`).
pub fn scanline_overlay(painter: &egui::Painter, rect: egui::Rect, strength: f32, phase: f32) {
    let s = strength.clamp(0.0, 1.0);
    if s <= 0.0 || rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let p = painter.with_clip_rect(rect);

    // Scanlines.
    let line_h = 3.0;
    let dark = Color32::from_black_alpha((26.0 * s) as u8);
    let mut y = rect.top() + phase.rem_euclid(line_h);
    while y < rect.bottom() {
        p.hline(rect.x_range(), y, Stroke::new(1.0, dark));
        y += line_h;
    }

    // Faint noise.
    let step = 10.0;
    let light = Color32::from_white_alpha((16.0 * s) as u8);
    let threshold = (10.0 * s) as u64;
    let mut yy = rect.top();
    while yy < rect.bottom() {
        let mut xx = rect.left();
        while xx < rect.right() {
            if splitmix(hash2(xx, yy, phase)) % 100 < threshold {
                p.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(xx, yy), Vec2::splat(1.5)),
                    CornerRadius::ZERO,
                    light,
                );
            }
            xx += step;
        }
        yy += step;
    }
}

/// Slight positional jitter for hover/active states. `amount` in px.
/// Deterministic: the same `seed` always yields the same offset.
pub fn glitch_offset(seed: u64, amount: f32) -> egui::Vec2 {
    if amount <= 0.0 {
        return Vec2::ZERO;
    }
    let x = unit(splitmix(seed));
    let y = unit(splitmix(seed ^ 0x9E37_79B9_7F4A_7C15));
    egui::vec2((x * 2.0 - 1.0) * amount, (y * 2.0 - 1.0) * amount)
}

// ---------------------------------------------------------------------------
// internals
// ---------------------------------------------------------------------------

fn widget(bg: Color32, fg: Color32, stroke: Color32, cr: CornerRadius) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke: Stroke::new(1.0, stroke),
        corner_radius: cr,
        fg_stroke: Stroke::new(1.0, fg),
        expansion: 0.0,
    }
}

fn text_styles() -> BTreeMap<TextStyle, FontId> {
    [
        (TextStyle::Small, FontId::new(10.0, FontFamily::Monospace)),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(20.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
    ]
    .into()
}

fn palette_id() -> egui::Id {
    egui::Id::new("glitch_studio::theme::palette")
}

fn current_palette(ctx: &egui::Context) -> Palette {
    ctx.data(|d| d.get_temp::<Palette>(palette_id()))
        .unwrap_or_else(|| Palette::builtin(builtins::DEFAULT_ID, false))
}

/// FNV-1a over the bytes of `s`.
fn hash_str(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// splitmix64 — a fast, well-distributed integer mixer.
fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Top 24 bits of `v` mapped to `[0, 1)`.
fn unit(v: u64) -> f32 {
    let bits = ((v >> 40) & 0x00FF_FFFF) as u32;
    bits as f32 / 16_777_216.0
}

fn hash2(x: f32, y: f32, phase: f32) -> u64 {
    let xi = u64::from(x.to_bits());
    let yi = u64::from(y.to_bits());
    let pi = u64::from(phase.to_bits());
    xi ^ yi.rotate_left(21) ^ pi.rotate_left(42)
}
