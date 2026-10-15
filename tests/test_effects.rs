//! Контрактные тесты для всех эффектов: детерминизм, безопасность на границах,
//! устойчивость к размерам. Эти тементы обязаны ломать сборку, если эффект паникует.

use glitch_studio::core::effect::EffectCtx;
use glitch_studio::core::frame::Frame;
use glitch_studio::core::params::ParamSpec;
use glitch_studio::core::registry::Registry;
use glitch_studio::core::seed::Rng;

fn make(seed: u64) -> Rng {
    Rng::new(seed)
}

/// Кадры самых разных размеров, включая вырожденные.
fn sizes() -> Vec<(u32, u32)> {
    vec![
        (1, 1),
        (1, 7),
        (7, 1),
        (2, 2),
        (3, 5),
        (16, 16),
        (17, 9),
        (64, 48),
        (127, 3),
        (320, 240),
    ]
}

/// Градиент с высокой детализацией: однотонный кадр скрывает баги.
fn sample(w: u32, h: u32) -> Frame {
    let mut f = Frame::new(w, h);
    for y in 0..h {
        for x in 0..w {
            f.set(
                x as i64,
                y as i64,
                [
                    (x * 7 % 256) as u8,
                    (y * 13 % 256) as u8,
                    ((x * y) % 256) as u8,
                    255,
                ],
            );
        }
    }
    f
}


/// Значения, при которых эффект обязан быть виден.
///
/// Усиливаем только «силовые» ползунки: amount, intensity, level, density.
/// Углы, направления и битовые глубины не трогаем — angle=216 даёт нулевое
/// смещение, bits=8 означает «порчи нет».
fn energized(spec: ParamSpec) -> f32 {
    const POWER: [&str; 5] = ["amount", "intensity", "level", "strength", "density"];
    if POWER.iter().any(|p| spec.key.contains(p)) && spec.max > spec.min {
        let target = spec.min + (spec.max - spec.min) * 0.8;
        if target > spec.min {
            return target;
        }
    }
    spec.default
}

/// Кадр, отличный от исходного. История из копий того же кадра не годится:
/// temporal-эффекты смешивают кадр с предыдущим, и результат совпадёт с оригиналом.
fn disturbed(f: &Frame) -> std::sync::Arc<Frame> {
    let mut d = f.clone();
    for (i, px) in d.data.chunks_exact_mut(4).enumerate() {
        px[0] = 255 - px[0];
        px[1] = px[1].wrapping_add((i % 97) as u8);
        px[2] = 255 - px[2];
    }
    std::sync::Arc::new(d)
}

fn run(eff: &dyn glitch_studio::core::Effect, input: &Frame, seed: u64, extreme: bool) -> Frame {
    let specs = eff.params();
    let mut values = eff.defaults();

    if extreme {
        // Прогоняем каждое значение через min / max / NaN / inf,
        // чтобы эффект гарантированно встретил невозможные входные данные.
        for (i, spec) in specs.iter().enumerate() {
            let v = match i % 4 {
                0 => spec.min,
                1 => spec.max,
                2 => f32::NAN,
                _ => f32::INFINITY,
            };
            values.set(i, v);
        }
    }

    let mut rng = make(seed);
    let mut frame = input.clone();
    let prev: Vec<std::sync::Arc<Frame>> = vec![disturbed(input)];
    let mut ctx = EffectCtx {
        frame: &mut frame,
        values: &values,
        rng: &mut rng,
        chaos: 1.0,
        prev: &prev,
    };
    eff.apply(&mut ctx)
        .unwrap_or_else(|e| panic!("эффект {} вернул ошибку на штатных данных: {e}", eff.id()));
    frame
}

#[test]
fn every_effect_keeps_frame_dimensions() {
    let reg = Registry::new();
    for eff in reg.all() {
        for (w, h) in sizes() {
            let input = sample(w, h);
            let out = run(eff.as_ref(), &input, 42, false);
            assert_eq!(
                (out.width, out.height),
                (w, h),
                "эффект {} изменил размер {w}x{h}",
                eff.id()
            );
            assert_eq!(
                out.data.len(),
                (w * h * 4) as usize,
                "эффект {} нарушил длину буфера на {w}x{h}",
                eff.id()
            );
        }
    }
}

#[test]
fn every_effect_is_deterministic_for_same_seed() {
    let reg = Registry::new();
    for eff in reg.all() {
        let input = sample(48, 36);
        let a = run(eff.as_ref(), &input, 1234, false);
        let b = run(eff.as_ref(), &input, 1234, false);
        assert_eq!(
            a.data,
            b.data,
            "эффект {} не детерминирован по seed",
            eff.id()
        );
    }
}

#[test]
fn different_seeds_give_different_results() {
    let reg = Registry::new();
    let input = sample(48, 36);
    let mut differing = Vec::new();
    let mut identical = Vec::new();

    for eff in reg.all() {
        let a = run(eff.as_ref(), &input, 1, false);
        let b = run(eff.as_ref(), &input, 2, false);
        // Часть эффектов по своей природе не зависит от seed
        // (инверсия, скан-линии) — это нормально, фиксируем и не падаем.
        if a.data == b.data {
            identical.push(eff.id());
        } else {
            differing.push(eff.id());
        }
    }
    assert!(
        differing.len() >= 10,
        "слишком мало эффектов зависят от seed: {differing:?}"
    );
    println!("зависят от seed: {} шт", differing.len());
    println!("не зависят от seed: {identical:?}");
}

#[test]
fn every_effect_survives_slider_extremes() {
    let reg = Registry::new();
    for eff in reg.all() {
        for (w, h) in [(1u32, 1u32), (1, 9), (9, 1), (37, 23)] {
            let input = sample(w, h);
            let out = run(eff.as_ref(), &input, 7, true);
            assert_eq!(
                (out.width, out.height),
                (w, h),
                "эффект {} сломался на крайних значениях {w}x{h}",
                eff.id()
            );
        }
    }
}

#[test]
fn every_effect_survives_empty_history() {
    let reg = Registry::new();
    for eff in reg.all() {
        let input = sample(32, 24);
        let mut values = eff.defaults();
        for i in 0..values.len() {
            values.set(i, eff.params()[i].max);
        }
        let mut rng = make(5);
        let mut frame = input.clone();
        let mut ctx = EffectCtx {
            frame: &mut frame,
            values: &values,
            rng: &mut rng,
            chaos: 1.0,
            prev: &[],
        };
        eff.apply(&mut ctx)
            .unwrap_or_else(|e| panic!("эффект {} упал без истории: {e}", eff.id()));
        assert_eq!((frame.width, frame.height), (32, 24));
    }
}

#[test]
fn every_effect_handles_mismatched_history_size() {
    let reg = Registry::new();
    let tiny_history = vec![std::sync::Arc::new(sample(4, 4))];
    for eff in reg.all() {
        let input = sample(64, 48);
        let mut rng = make(9);
        let mut frame = input.clone();
        let mut ctx = EffectCtx {
            frame: &mut frame,
            values: &eff.defaults(),
            rng: &mut rng,
            chaos: 1.0,
            prev: &tiny_history,
        };
        eff.apply(&mut ctx)
            .unwrap_or_else(|e| panic!("эффект {} упал при размере истории: {e}", eff.id()));
        assert_eq!((frame.width, frame.height), (64, 48));
    }
}

#[test]
fn every_effect_works_with_zero_chaos() {
    let reg = Registry::new();
    for eff in reg.all() {
        let input = sample(32, 32);
        let mut rng = make(3);
        let mut frame = input.clone();
        let mut ctx = EffectCtx {
            frame: &mut frame,
            values: &eff.defaults(),
            rng: &mut rng,
            chaos: 0.0,
            prev: &[],
        };
        eff.apply(&mut ctx)
            .unwrap_or_else(|e| panic!("эффект {} упал при chaos=0: {e}", eff.id()));
        assert_eq!((frame.width, frame.height), (32, 32));
    }
}

#[test]
fn every_effect_has_readable_metadata() {
    let reg = Registry::new();
    let mut ids = std::collections::HashSet::new();
    for eff in reg.all() {
        assert!(!eff.id().is_empty(), "пустой id");
        assert!(ids.insert(eff.id()), "дублирующийся id: {}", eff.id());
        assert!(
            eff.id()
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "id должен быть snake_case: {}",
            eff.id()
        );
        assert!(!eff.name().is_empty(), "{} без имени", eff.id());
        assert!(
            !eff.hint().is_empty(),
            "{} без подсказки — UI покажет пустоту",
            eff.id()
        );
        assert!(
            eff.params().len() >= 2,
            "у эффекта {} меньше двух ползунков",
            eff.id()
        );
        for spec in eff.params() {
            assert!(!spec.key.is_empty(), "{}: пустой key", eff.id());
            assert!(!spec.label.is_empty(), "{}: пустой label", eff.id());
            assert!(
                spec.min < spec.max,
                "{} / {}: min >= max ({}..{})",
                eff.id(),
                spec.key,
                spec.min,
                spec.max
            );
            assert!(
                spec.default >= spec.min && spec.default <= spec.max,
                "{} / {}: дефолт {} вне диапазона {}..{}",
                eff.id(),
                spec.key,
                spec.default,
                spec.min,
                spec.max
            );
            assert!(
                spec.step >= 0.0,
                "{} / {}: отрицательный шаг",
                eff.id(),
                spec.key
            );
        }
    }
    assert!(
        reg.len() >= 25,
        "в требованиях минимум 25 эффектов, а есть {}",
        reg.len()
    );
}

#[test]
fn every_effect_has_unique_param_keys() {
    let reg = Registry::new();
    for eff in reg.all() {
        let mut keys = std::collections::HashSet::new();
        for spec in eff.params() {
            assert!(
                keys.insert(spec.key),
                "{}: ключ параметра повторяется — {}",
                eff.id(),
                spec.key
            );
        }
    }
}

#[test]
fn every_effect_actually_changes_something() {
    let reg = Registry::new();
    let input = sample(64, 64);
    let history = disturbed(&input);
    let mut inert = Vec::new();

    for eff in reg.all() {
        let mut values = eff.defaults();
        for i in 0..values.len() {
            values.set(i, energized(eff.params()[i]));
        }

        // Часть эффектов вероятностная (frame_stutter держит кадр с шансом
        // `hold`), поэтому одного прогона мало: даём восемь seed'ов и требуем
        // хотя бы одного срабатывания.
        let mut changed = false;
        for attempt in 0..8u64 {
            let mut rng = make(11 + attempt * 977);
            let mut frame = input.clone();
            let mut ctx = EffectCtx {
                frame: &mut frame,
                values: &values,
                rng: &mut rng,
                chaos: 1.0,
                prev: std::slice::from_ref(&history),
            };
            eff.apply(&mut ctx).expect("эффект не должен падать");
            if frame.data != input.data {
                changed = true;
                break;
            }
        }
        if !changed {
            inert.push(eff.id());
        }
    }

    assert!(
        inert.is_empty(),
        "эффекты не сработали ни на одном из восьми seed'ов: {inert:?}"
    );
}

#[test]
fn big_image_does_not_panic() {
    let reg = Registry::new();
    let input = sample(640, 480);
    for eff in reg.all() {
        let out = run(eff.as_ref(), &input, 77, false);
        assert_eq!(
            out.data.len(),
            640 * 480 * 4,
            "крупный кадр сломал {}",
            eff.id()
        );
    }
}

#[test]
fn full_pipeline_is_deterministic() {
    let reg = Registry::new();
    let input = sample(80, 60);
    let mut p = glitch_studio::core::Pipeline::with_defaults(9001, &reg);
    for e in p.stack.iter_mut() {
        e.enabled = true;
    }
    let a = p.apply(&input, &reg, &[]).unwrap();
    let b = p.apply(&input, &reg, &[]).unwrap();
    assert_eq!(a.data, b.data, "весь пайплайн обязан быть детерминирован");
    assert_eq!(a.width, 80);
    assert_eq!(a.height, 60);
}

#[test]
fn builtin_presets_all_apply() {
    let reg = Registry::new();
    let presets = glitch_studio::core::builtin_presets::builtin_presets();
    assert!(
        presets.len() >= 10,
        "в требованиях минимум 10 пресетов, есть {}",
        presets.len()
    );
    let input = sample(64, 48);
    for preset in presets {
        let name = preset.name.clone();
        let pipeline = preset.into_pipeline(&reg);
        let enabled = pipeline.stack.iter().filter(|e| e.enabled).count();
        assert!(
            enabled > 0,
            "пресет «{}» не включает ни одного эффекта — все id разошлись",
            name
        );
        let out = pipeline
            .apply(&input, &reg, &[])
            .unwrap_or_else(|e| panic!("пресет «{name}» упал: {e}"));
        assert_eq!((out.width, out.height), (64, 48));
        assert_ne!(
            out.data, input.data,
            "пресет «{}» не изменил картинку",
            name
        );
    }
}

#[test]
fn preset_names_are_unique_and_resolvable() {
    let presets = glitch_studio::core::builtin_presets::builtin_presets();
    let mut names = std::collections::HashSet::new();
    for p in &presets {
        assert!(names.insert(p.name.clone()), "дубль названия: {}", p.name);
        assert!(
            glitch_studio::core::builtin_presets::resolve(&p.name, &[]).is_some(),
            "пресет «{}» не находится по имени",
            p.name
        );
    }
    assert!(
        glitch_studio::core::builtin_presets::resolve("нет такого", &[]).is_none(),
        "несуществующий пресет обязан вернуть None"
    );
}
