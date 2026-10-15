//! Material You tonal palette.
//!
//! A single seed color is expanded into the full 22-role color set the app
//! uses. The expansion is delegated to the `material-colors` crate, which
//! implements Google's HCT/CAM16 tone mapping — so contrast between a role and
//! its `on_*` counterpart is perceptually correct instead of hand-tuned.

use egui::Color32;
use material_colors::color::Rgb;
use material_colors::dynamic_color::Variant;
use material_colors::scheme::Scheme;
use material_colors::theme::ThemeBuilder;

/// A resolved color set for the whole app.
///
/// Every field is a Material You color role. Widgets must read colors from
/// here (via [`crate::theme::visuals`]) and never hardcode one.
#[derive(Debug, Clone)]
pub struct Palette {
    pub primary: egui::Color32,
    pub on_primary: egui::Color32,
    pub primary_container: egui::Color32,
    pub on_primary_container: egui::Color32,
    pub secondary: egui::Color32,
    pub on_secondary: egui::Color32,
    pub secondary_container: egui::Color32,
    pub on_secondary_container: egui::Color32,
    pub tertiary: egui::Color32,
    pub on_tertiary: egui::Color32,
    pub tertiary_container: egui::Color32,
    pub on_tertiary_container: egui::Color32,
    pub error: egui::Color32,
    pub background: egui::Color32,
    pub on_background: egui::Color32,
    pub surface: egui::Color32,
    pub on_surface: egui::Color32,
    pub surface_variant: egui::Color32,
    pub on_surface_variant: egui::Color32,
    pub surface_container_low: egui::Color32,
    pub surface_container: egui::Color32,
    pub surface_container_high: egui::Color32,
    pub outline: egui::Color32,
    pub is_light: bool,
}

impl Palette {
    /// Built-in theme by name, e.g. `"Violet Dark"`. Unknown name -> Violet Dark.
    pub fn builtin(name: &str, light: bool) -> Palette {
        crate::theme::builtins::build(name, light)
    }

    /// Palette derived from an arbitrary seed color (Custom Color mode).
    pub fn from_seed_rgb(r: u8, g: u8, b: u8, light: bool) -> Palette {
        Self::from_seed_variant((r, g, b), Variant::TonalSpot, light)
    }

    /// Palette derived from pixel data (From Image mode).
    ///
    /// Extracts the dominant hue from the buffer and expands it. Falls back to
    /// the default theme when the buffer has no usable color.
    pub fn from_pixels(pixels: &[u8], light: bool) -> Palette {
        match crate::theme::dynamic::dominant_seed(pixels) {
            Some(seed) => Self::from_seed_variant(seed, Variant::Vibrant, light),
            None => Self::builtin("Violet Dark", light),
        }
    }

    /// Core generator: seed + scheme variant -> resolved palette.
    pub(crate) fn from_seed_variant(seed: (u8, u8, u8), variant: Variant, light: bool) -> Palette {
        let theme = ThemeBuilder::with_source(rgb(seed))
            .variant(variant)
            .build();
        let scheme = if light {
            theme.schemes.light
        } else {
            theme.schemes.dark
        };
        Self::from_scheme(&scheme, light)
    }

    /// Maps a Material [`Scheme`] onto the app's role set.
    pub(crate) fn from_scheme(s: &Scheme, light: bool) -> Palette {
        Palette {
            primary: c32(s.primary),
            on_primary: c32(s.on_primary),
            primary_container: c32(s.primary_container),
            on_primary_container: c32(s.on_primary_container),
            secondary: c32(s.secondary),
            on_secondary: c32(s.on_secondary),
            secondary_container: c32(s.secondary_container),
            on_secondary_container: c32(s.on_secondary_container),
            tertiary: c32(s.tertiary),
            on_tertiary: c32(s.on_tertiary),
            tertiary_container: c32(s.tertiary_container),
            on_tertiary_container: c32(s.on_tertiary_container),
            error: c32(s.error),
            background: c32(s.background),
            on_background: c32(s.on_background),
            surface: c32(s.surface),
            on_surface: c32(s.on_surface),
            surface_variant: c32(s.surface_variant),
            on_surface_variant: c32(s.on_surface_variant),
            surface_container_low: c32(s.surface_container_low),
            surface_container: c32(s.surface_container),
            surface_container_high: c32(s.surface_container_high),
            outline: c32(s.outline),
            is_light: light,
        }
    }
}

/// `(r, g, b)` -> material `Rgb`.
pub(crate) fn rgb((r, g, b): (u8, u8, u8)) -> Rgb {
    Rgb::new(r, g, b)
}

/// material `Rgb` -> egui `Color32`.
pub(crate) fn c32(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.red, rgb.green, rgb.blue)
}
