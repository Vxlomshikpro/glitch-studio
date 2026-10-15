//! Встроенные пресеты: 10 заводных наборов поверх полного стека эффектов.

use crate::core::pipeline::{Pipeline, StackEntry};
use crate::core::preset::Preset;
use crate::core::registry::Registry;

/// Шаг пресета: id эффекта, его mix, точечные значения по ключам параметров.
type Step = (&'static str, f32, &'static [(&'static str, f32)]);

fn build(name: &str, registry: &Registry, steps: &[Step]) -> Preset {
    let mut pipeline = Pipeline::with_defaults(0xC0FFEE, registry);

    for entry in pipeline.stack.iter_mut() {
        entry.enabled = false;
        entry.mix = 1.0;
    }

    for (id, mix, pairs) in steps {
        let Some(entry) = pipeline.stack.iter_mut().find(|e| e.id == *id) else {
            continue;
        };
        let Ok(specs) = registry.params_of(id) else {
            continue;
        };
        entry.enabled = true;
        entry.mix = *mix;
        entry.values.reconcile(specs);
        for (k, v) in *pairs {
            if let Some(pos) = specs.iter().position(|s| s.key == *k) {
                entry.values.set(pos, *v);
            }
        }
    }

    Preset::builtin(name, pipeline)
}

/// 10 заводных пресетов. Реестр обязателен: пресеты ссылаются на id эффектов.
pub fn builtin_presets() -> Vec<Preset> {
    let reg = Registry::new();

    let vhs: Step = ("rgb_split", 1.0, &[("amount", 8.0), ("angle", 12.0)]);
    let slice: Step = ("slice_shift", 0.85, &[("amount", 45.0), ("density", 30.0)]);
    let bleed: Step = ("color_bleed", 0.7, &[("amount", 60.0)]);
    let scan: Step = ("scanlines", 0.5, &[("intensity", 40.0)]);
    let track: Step = ("tracking_error", 0.6, &[("amount", 30.0)]);
    let noise: Step = ("noise", 0.22, &[("amount", 22.0)]);

    vec![
        build(
            "VHS Death",
            &reg,
            &[
                vhs,
                slice,
                bleed,
                scan,
                track,
                noise,
                ("hue_shift", 0.3, &[("angle", 8.0)]),
            ],
        ),
        build(
            "Datamosh Hell",
            &reg,
            &[
                (
                    "block_displacement",
                    1.0,
                    &[("amount", 60.0), ("block_size", 34.0)],
                ),
                ("databend", 0.8, &[("level", 55.0)]),
                ("jpeg_crush", 0.6, &[("passes", 6.0), ("quality", 18.0)]),
                ("bit_crush", 0.5, &[("bits", 3.0)]),
            ],
        ),
        build(
            "Cyber Slice",
            &reg,
            &[
                ("slice_shift", 1.0, &[("amount", 90.0), ("density", 22.0)]),
                ("scan_jitter", 0.8, &[("amount", 40.0)]),
                ("posterize", 0.4, &[("levels", 5.0)]),
                ("rgb_split", 0.5, &[("amount", 14.0)]),
            ],
        ),
        build(
            "Pixel Rain",
            &reg,
            &[
                ("pixel_sort", 1.0, &[("amount", 70.0), ("length", 90.0)]),
                ("pixelate", 0.35, &[("size", 7.0)]),
                ("dither", 0.3, &[("amount", 30.0)]),
                ("wave", 0.3, &[("amplitude", 5.0)]),
            ],
        ),
        build(
            "Analog Rot",
            &reg,
            &[
                ("vignette", 0.6, &[("intensity", 55.0)]),
                ("bloom", 0.45, &[("threshold", 60.0), ("intensity", 40.0)]),
                ("grain", 0.5, &[("amount", 35.0)]),
                ("hue_shift", 0.3, &[("angle", 18.0)]),
            ],
        ),
        build(
            "Sunset Bleed",
            &reg,
            &[
                ("color_bleed", 1.0, &[("amount", 85.0)]),
                ("hue_shift", 0.7, &[("angle", 200.0)]),
                ("wave", 0.5, &[("amplitude", 14.0)]),
                ("bloom", 0.3, &[("intensity", 30.0)]),
            ],
        ),
        build(
            "CRT Dead",
            &reg,
            &[
                ("crt", 1.0, &[("intensity", 70.0)]),
                ("scanlines", 0.8, &[("intensity", 65.0)]),
                ("tracking_error", 0.5, &[("amount", 45.0)]),
                ("vignette", 0.4, &[("intensity", 40.0)]),
            ],
        ),
        build(
            "Ghost Machine",
            &reg,
            &[
                ("ghosting", 0.8, &[("amount", 50.0)]),
                ("channel_swap", 0.5, &[("amount", 60.0)]),
                ("invert", 0.15, &[("amount", 20.0)]),
                ("rgb_split", 0.4, &[("amount", 6.0)]),
            ],
        ),
        build(
            "Static Soup",
            &reg,
            &[
                ("noise", 1.0, &[("amount", 60.0)]),
                ("static", 0.6, &[("amount", 45.0)]),
                ("bit_crush", 0.4, &[("bits", 4.0)]),
                ("dither", 0.3, &[("amount", 25.0)]),
            ],
        ),
        build(
            "Soft Corruption",
            &reg,
            &[
                ("dither", 0.7, &[("amount", 40.0)]),
                ("posterize", 0.5, &[("levels", 8.0)]),
                ("wave", 0.35, &[("amplitude", 6.0)]),
                ("grain", 0.3, &[("amount", 18.0)]),
            ],
        ),
    ]
}

/// Встроенный пресет по имени.
pub fn find_builtin(name: &str) -> Option<Preset> {
    builtin_presets().into_iter().find(|p| p.name == name)
}

/// Пресет по имени: сначала пользовательские, потом встроенные.
pub fn resolve(name: &str, user: &[Preset]) -> Option<Preset> {
    user.iter()
        .find(|p| p.name == name)
        .cloned()
        .or_else(|| find_builtin(name))
}

/// Стек из одного эффекта — для быстрого применения кнопкой.
pub fn solo(id: &str, registry: &Registry, mix: f32) -> Pipeline {
    let mut p = Pipeline::new(0xC0FFEE);
    let values = registry.get(id).map(|e| e.defaults()).unwrap_or_default();
    p.stack.push(StackEntry::new(id, values));
    p.stack[0].mix = mix;
    p
}
