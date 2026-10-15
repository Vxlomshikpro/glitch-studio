//! The ten built-in themes.
//!
//! Each theme is a seed color plus a Material scheme [`Variant`] — the variant
//! is the theme's saturation/lightness "character". A few themes also pin
//! explicit secondary/tertiary seeds to get the neon glitchcore accents, and
//! `Mono Ghost` splices a single accent into an otherwise monochrome scheme.
//! The full 22-role palette is generated in [`super::palette`]; nothing here
//! hand-writes individual colors.

use material_colors::dynamic_color::Variant;
use material_colors::theme::ThemeBuilder;

use super::palette::{c32, rgb, Palette};

/// One built-in theme definition.
pub(crate) struct ThemeDef {
    /// Internal id, e.g. `"Violet Dark"`.
    pub id: &'static str,
    /// Russian display label.
    pub label: &'static str,
    /// Seed color the whole palette is derived from.
    pub seed: (u8, u8, u8),
    /// Material scheme variant — the theme's saturation/lightness character.
    pub variant: Variant,
    /// Optional explicit secondary seed (neon accents).
    pub secondary: Option<(u8, u8, u8)>,
    /// Optional explicit tertiary seed.
    pub tertiary: Option<(u8, u8, u8)>,
    /// Optional accent spliced into the primary role (Mono Ghost).
    pub accent: Option<(u8, u8, u8)>,
}

/// The default theme id — also the fallback for unknown names.
pub(crate) const DEFAULT_ID: &str = "Violet Dark";

const DEFS: &[ThemeDef] = &[
    ThemeDef {
        id: "Violet Dark",
        label: "Фиолетовый",
        seed: (0xb3, 0x9d, 0xff),
        variant: Variant::TonalSpot,
        secondary: Some((0x9a, 0x7d, 0xff)),
        tertiary: Some((0xff, 0x8a, 0xd8)),
        accent: None,
    },
    ThemeDef {
        id: "Cyber Pink",
        label: "Кибер-розовый",
        seed: (0xff, 0x2d, 0x95),
        variant: Variant::Vibrant,
        secondary: Some((0xb0, 0x26, 0xff)),
        tertiary: Some((0x00, 0xe5, 0xff)),
        accent: None,
    },
    ThemeDef {
        id: "Toxic Green",
        label: "Токсичный зелёный",
        seed: (0x39, 0xff, 0x14),
        variant: Variant::Vibrant,
        secondary: Some((0xaa, 0xff, 0x00)),
        tertiary: Some((0x00, 0xff, 0x9d)),
        accent: None,
    },
    ThemeDef {
        id: "Ice Blue",
        label: "Ледяной синий",
        seed: (0x4f, 0xc3, 0xf7),
        variant: Variant::TonalSpot,
        secondary: Some((0x7c, 0x9c, 0xff)),
        tertiary: Some((0x00, 0xe0, 0xd0)),
        accent: None,
    },
    ThemeDef {
        id: "Blood Red",
        label: "Кровавый красный",
        seed: (0xe5, 0x39, 0x35),
        variant: Variant::Vibrant,
        secondary: Some((0xff, 0x52, 0x52)),
        tertiary: Some((0xff, 0x8a, 0x65)),
        accent: None,
    },
    ThemeDef {
        id: "Sunset",
        label: "Закат",
        seed: (0xff, 0x6d, 0x3a),
        variant: Variant::Vibrant,
        secondary: Some((0xff, 0x2d, 0x95)),
        tertiary: Some((0xb0, 0x26, 0xff)),
        accent: None,
    },
    ThemeDef {
        id: "Mono Ghost",
        label: "Моно-призрак",
        seed: (0x9e, 0x9e, 0x9e),
        variant: Variant::Monochrome,
        secondary: None,
        tertiary: None,
        accent: Some((0xb0, 0xb0, 0xff)),
    },
    ThemeDef {
        id: "Amber Terminal",
        label: "Янтарный терминал",
        seed: (0xff, 0xb0, 0x00),
        variant: Variant::TonalSpot,
        secondary: Some((0xff, 0x8c, 0x00)),
        tertiary: Some((0xff, 0xd5, 0x4f)),
        accent: None,
    },
    ThemeDef {
        id: "Vapor",
        label: "Вейпорвейв",
        seed: (0x00, 0xe5, 0xd0),
        variant: Variant::Vibrant,
        secondary: Some((0xff, 0x71, 0xce)),
        tertiary: Some((0xb9, 0x67, 0xff)),
        accent: None,
    },
    ThemeDef {
        id: "Light Pastel",
        label: "Светлая пастель",
        seed: (0xc8, 0xb6, 0xff),
        variant: Variant::TonalSpot,
        secondary: Some((0xff, 0xd6, 0xe0)),
        tertiary: Some((0xbd, 0xe0, 0xfe)),
        accent: None,
    },
];

/// All theme ids in display order.
pub fn theme_names() -> Vec<String> {
    DEFS.iter().map(|d| d.id.to_string()).collect()
}

/// `(id, russian label)` pairs in display order.
pub fn builtin_themes() -> Vec<(String, String)> {
    DEFS.iter()
        .map(|d| (d.id.to_string(), d.label.to_string()))
        .collect()
}

/// Builds the palette for a theme id. Unknown id falls back to the default.
pub(crate) fn build(name: &str, light: bool) -> Palette {
    match DEFS.iter().find(|d| d.id == name) {
        Some(def) => build_def(def, light),
        None => Palette::from_seed_variant((0xb3, 0x9d, 0xff), Variant::TonalSpot, light),
    }
}

fn build_def(def: &ThemeDef, light: bool) -> Palette {
    let mut builder = ThemeBuilder::with_source(rgb(def.seed)).variant(def.variant);
    if let Some(s) = def.secondary {
        builder = builder.secondary(rgb(s));
    }
    if let Some(t) = def.tertiary {
        builder = builder.tertiary(rgb(t));
    }
    let theme = builder.build();
    let scheme = if light {
        theme.schemes.light
    } else {
        theme.schemes.dark
    };
    let mut pal = Palette::from_scheme(&scheme, light);

    // Mono Ghost: keep the monochrome surfaces but give the primary role a
    // single real accent so the UI still has one point of color.
    if let Some(accent) = def.accent {
        let acc = ThemeBuilder::with_source(rgb(accent))
            .variant(Variant::TonalSpot)
            .build();
        let asch = if light {
            acc.schemes.light
        } else {
            acc.schemes.dark
        };
        pal.primary = c32(asch.primary);
        pal.on_primary = c32(asch.on_primary);
        pal.primary_container = c32(asch.primary_container);
        pal.on_primary_container = c32(asch.on_primary_container);
    }

    pal
}
