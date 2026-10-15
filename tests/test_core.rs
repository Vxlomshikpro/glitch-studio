//! Ядро: кадр, параметры, пайплайн, пресеты. Эффектов не касается.

use glitch_studio::core::frame::Frame;
use glitch_studio::core::params::{ParamSpec, ParamValues};
use glitch_studio::core::pipeline::{Pipeline, StackEntry};
use glitch_studio::core::preset::Preset;
use glitch_studio::core::seed::Rng;

fn grad(w: u32, h: u32) -> Frame {
    let mut f = Frame::new(w, h);
    for y in 0..h {
        for x in 0..w {
            f.set(
                x as i64,
                y as i64,
                [
                    (x * 255 / w.max(1)) as u8,
                    (y * 255 / h.max(1)) as u8,
                    ((x + y) * 255 / (w + h).max(1)) as u8,
                    255,
                ],
            );
        }
    }
    f
}

#[test]
fn frame_from_rgba_checks_length() {
    let ok = Frame::from_rgba(2, 2, vec![0; 16]);
    assert!(ok.is_some());
    assert!(Frame::from_rgba(2, 2, vec![0; 15]).is_none());
    assert!(Frame::from_rgba(0, 0, vec![]).is_some());
}

#[test]
fn frame_set_out_of_bounds_is_noop() {
    let mut f = Frame::new(4, 4);
    for c in 0..4 {
        f.data[c] = 7;
    }
    f.set(-1, 0, [1, 2, 3, 4]);
    f.set(0, -1, [1, 2, 3, 4]);
    f.set(4, 0, [1, 2, 3, 4]);
    f.set(0, 4, [1, 2, 3, 4]);
    assert_eq!(f.data[0], 7, "пиксель за границей изменил кадр");
    assert!(f.get(-1, -1).is_none());
}

#[test]
fn frame_1x1_survives_every_op() {
    let mut f = Frame::new(1, 1);
    f.set(0, 0, [10, 20, 30, 255]);
    assert_eq!(f.get(0, 0), Some([10, 20, 30, 255]));
    f.blend_from(&Frame::new(1, 1), 1.0);
    assert_eq!(f.get(0, 0), Some([0, 0, 0, 0]));
}

#[test]
fn blend_from_mixed_sizes_is_noop() {
    let mut a = Frame::new(2, 2);
    a.data.fill(9);
    let other = Frame::new(3, 3);
    a.blend_from(&other, 0.5);
    assert!(a.data.iter().all(|v| *v == 9), "разный размер изменил кадр");
}

#[test]
fn blend_is_linear() {
    let mut a = Frame::new(1, 1);
    let mut b = Frame::new(1, 1);
    a.data = vec![0, 0, 0, 255];
    b.data = vec![100, 200, 40, 255];
    a.blend_from(&b, 0.5);
    assert_eq!(a.get(0, 0), Some([50, 100, 20, 255]));
}

#[test]
fn sample_bilinear_never_panics_on_edges() {
    let f = grad(3, 3);
    for x in [-100.0, 0.0, 1.5, 2.0, 100.0] {
        for y in [-100.0, 0.0, 1.5, 2.0, 100.0] {
            let _ = f.sample_bilinear(x, y);
        }
    }
}

#[test]
fn rng_is_deterministic_per_seed() {
    let mut a = Rng::new(12345);
    let mut b = Rng::new(12345);
    for _ in 0..64 {
        assert_eq!(a.below(1000), b.below(1000));
    }
}

#[test]
fn rng_differs_between_seeds() {
    let mut a = Rng::new(1);
    let mut b = Rng::new(2);
    let va: Vec<u32> = (0..32).map(|_| a.below(1_000_000)).collect();
    let vb: Vec<u32> = (0..32).map(|_| b.below(1_000_000)).collect();
    assert_ne!(va, vb, "разные seed дали одинаковую последовательность");
}

#[test]
fn rng_below_zero_and_one() {
    let mut r = Rng::new(7);
    assert_eq!(r.below(0), 0);
    assert_eq!(r.below(1), 0);
    for _ in 0..1000 {
        let u = r.unit();
        assert!((0.0..1.0).contains(&u), "unit() вышел за [0,1): {u}");
        let i = r.range_i(-3, 3);
        assert!((-3..=3).contains(&i), "range_i вышел за границы: {i}");
        let n = r.range(-5.0, 5.0);
        assert!((-5.0..=5.0).contains(&n), "range вышел за границы: {n}");
    }
}

#[test]
fn subseed_does_not_consume_parent_stream_inconsistently() {
    let mut a = Rng::new(99);
    let mut b = Rng::new(99);
    let s1 = a.subseed(0);
    let s2 = a.subseed(1);
    let t1 = b.subseed(0);
    let t2 = b.subseed(1);
    let mut s1 = s1;
    let mut s2 = s2;
    let mut t1 = t1;
    let mut t2 = t2;
    for _ in 0..16 {
        assert_eq!(s1.below(999), t1.below(999));
        assert_eq!(s2.below(999), t2.below(999));
    }
}

#[test]
fn params_defaults_and_reconcile() {
    static SPECS: [ParamSpec; 3] = [
        ParamSpec::new("a", "А", 0.0, 10.0, 5.0, 1.0),
        ParamSpec::new("b", "Б", 0.0, 1.0, 0.5, 0.1),
        ParamSpec::new("c", "В", -100.0, 100.0, 0.0, 5.0),
    ];
    let v = ParamValues::defaults_from(&SPECS);
    assert_eq!(v.len(), 3);
    assert_eq!(v.get(0), 5.0);
    assert_eq!(v.get(99), 0.0, "плохой индекс обязан давать 0.0");

    let mut changed = v.clone();
    changed.set(0, 3.0);
    changed.set(1, 0.25);
    changed.set(2, -50.0);

    let shrunk = changed.clone();
    let mut shorter = shrunk;
    shorter.values.truncate(1);
    shorter.reconcile(&SPECS);
    assert_eq!(shorter.len(), 3);
    assert_eq!(shorter.get(0), 3.0, "существующее значение потерялось");
    assert_eq!(shorter.get(2), 0.0, "недостающее значение не взяло дефолт");

    let mut wild = changed;
    wild.set(0, 1e9);
    wild.reconcile(&SPECS);
    assert_eq!(wild.get(0), 10.0, "значение не зажато в диапазон");
}

#[test]
fn param_formatting() {
    let s_int = ParamSpec::new("i", "Целый", 0.0, 100.0, 0.0, 1.0);
    assert_eq!(s_int.format(12.4), "12");
    let s_dec = ParamSpec::new("d", "Дробный", 0.0, 100.0, 0.0, 0.1);
    assert_eq!(s_dec.format(12.44), "12.4");
    let s_fine = ParamSpec::new("f", "Тонкий", 0.0, 1.0, 0.0, 0.001);
    assert_eq!(s_fine.format(0.5), "0.500");
    assert_eq!(s_int.format(-5.0), "0", "значение ниже минимума не зажато");
    assert_eq!(s_int.format(500.0), "100");
}

#[test]
fn empty_pipeline_is_identity() {
    let reg = glitch_studio::core::Registry::empty();
    let input = grad(32, 24);
    let out = Pipeline::new(1).apply(&input, &reg, &[]).unwrap();
    assert_eq!(out, input);
}

#[test]
fn pipeline_ignores_unknown_effect_id() {
    let reg = glitch_studio::core::Registry::empty();
    let mut p = Pipeline::new(1);
    p.stack
        .push(StackEntry::new("no_such_effect", ParamValues::default()));
    let input = grad(8, 8);
    let out = p.apply(&input, &reg, &[]).unwrap();
    assert_eq!(out, input, "неизвестный эффект не должен ломать пайплайн");
}

#[test]
fn pipeline_mix_zero_keeps_original() {
    struct Noop;
    impl glitch_studio::core::Effect for Noop {
        fn id(&self) -> &'static str {
            "test_noop"
        }
        fn name(&self) -> &'static str {
            "Noop"
        }
        fn group(&self) -> glitch_studio::core::EffectGroup {
            glitch_studio::core::EffectGroup::Color
        }
        fn params(&self) -> &'static [ParamSpec] {
            &[]
        }
        fn apply(
            &self,
            ctx: &mut glitch_studio::core::EffectCtx<'_>,
        ) -> Result<(), glitch_studio::core::GlitchError> {
            for px in ctx.frame.data.chunks_exact_mut(4) {
                px[0] = 0;
                px[1] = 0;
                px[2] = 0;
            }
            Ok(())
        }
    }

    let reg = glitch_studio::core::Registry::empty().with(Box::new(Noop));
    let input = grad(16, 16);

    let mut p = Pipeline::new(1);
    p.stack
        .push(StackEntry::new("test_noop", ParamValues::default()));
    p.stack[0].mix = 0.0;
    let out = p.apply(&input, &reg, &[]).unwrap();
    assert_eq!(out, input, "mix = 0 обязан оставить кадр нетронутым");

    p.stack[0].mix = 1.0;
    let out = p.apply(&input, &reg, &[]).unwrap();
    assert!(
        out.data.chunks_exact(4).all(|px| px[..3] == [0, 0, 0]),
        "mix = 1 обязан применить эффект"
    );
    assert!(
        out.data.chunks_exact(4).all(|px| px[3] == 255),
        "эффект не имел права трогать альфу"
    );
}

#[test]
fn pipeline_move_entry_reorders() {
    let mut p = Pipeline::new(1);
    for id in ["a", "b", "c"] {
        p.stack.push(StackEntry::new(id, ParamValues::default()));
    }
    p.move_entry(2, 0);
    let ids: Vec<&str> = p.stack.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["c", "a", "b"]);

    p.move_entry(99, 0);
    assert_eq!(p.stack.len(), 3, "выход за границы обязан быть no-op");
}

#[test]
fn pipeline_hash_changes_with_content() {
    let mut a = Pipeline::new(1);
    let mut b = Pipeline::new(1);
    assert_eq!(a.content_hash(), b.content_hash());
    b.chaos = 50.0;
    assert_ne!(a.content_hash(), b.content_hash());
    b.chaos = 1.0;
    a.chaos = 2.0;
    assert_ne!(a.content_hash(), b.content_hash(), "chaos участвует в хэше");
}

#[test]
fn pipeline_reconcile_clamps_everything() {
    let mut p = Pipeline::new(1);
    p.chaos = 9999.0;
    p.stack
        .push(StackEntry::new("ghosting", ParamValues::default()));
    p.stack[0].mix = 42.0;
    p.reconcile(&glitch_studio::core::Registry::new());
    assert_eq!(p.chaos, 100.0);
    assert_eq!(p.stack[0].mix, 1.0);
}

#[test]
fn preset_survives_json_roundtrip() {
    let reg = glitch_studio::core::Registry::new();
    let mut p = Pipeline::with_defaults(777, &reg);
    p.chaos = 66.0;
    if let Some(e) = p.stack.first_mut() {
        e.enabled = true;
        e.values.set(0, 42.5);
    }

    let preset = Preset::new("Тест", p.clone());
    let json = preset.to_json().unwrap();
    let back = Preset::from_json(&json).unwrap();
    assert_eq!(back.name, "Тест");
    assert_eq!(back.pipeline.seed, 777);
    assert_eq!(back.pipeline.chaos, 66.0);
    assert_eq!(back.pipeline.stack.len(), p.stack.len());
    assert_eq!(back.pipeline.stack[0].values.get(0), 42.5);
}

#[test]
fn preset_from_broken_json_is_error_not_panic() {
    assert!(Preset::from_json("{не json").is_err());
    assert!(Preset::from_json("").is_err());
}

#[test]
fn media_roundtrip_png_in_tmpdir() {
    let dir = std::env::temp_dir().join(format!("gs-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rt.png");

    let src = grad(37, 19);
    glitch_studio::core::media::save(&src, &path, 90).unwrap();
    let back = glitch_studio::core::media::load(&path).unwrap();

    assert_eq!(back.width, src.width);
    assert_eq!(back.height, src.height);
    assert_eq!(
        back.data, src.data,
        "PNG без потерь должен совпасть побайтно"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn media_next_export_path_never_overwrites() {
    let dir = std::env::temp_dir().join(format!("gs-exp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = glitch_studio::core::media::next_export_path(&dir, "photo", "png");
    std::fs::write(&a, b"x").unwrap();
    let b = glitch_studio::core::media::next_export_path(&dir, "photo", "png");
    assert_ne!(a, b);
    assert!(a
        .file_name()
        .unwrap()
        .to_string_lossy()
        .contains("glitch_001"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn media_missing_file_is_error() {
    assert!(glitch_studio::core::media::load(std::path::Path::new("/nope/nada.png")).is_err());
}

#[test]
fn downscale_keeps_aspect_and_bounds() {
    let big = grad(2000, 1000);
    let small = glitch_studio::core::media::downscale(&big, 100, 100);
    assert_eq!(small.width, 100);
    assert_eq!(small.height, 50);

    let already_small = grad(10, 10);
    let same = glitch_studio::core::media::downscale(&already_small, 100, 100);
    assert_eq!(same.width, 10);

    let tiny = Frame::new(1, 1);
    let t = glitch_studio::core::media::downscale(&tiny, 800, 800);
    assert_eq!((t.width, t.height), (1, 1));
}

#[test]
fn registry_lookup_and_unknown() {
    let reg = glitch_studio::core::Registry::new();
    assert!(
        reg.len() >= 25,
        "в реестре меньше 25 эффектов: {}",
        reg.len()
    );
    assert!(reg.get("rgb_split").is_ok());
    assert!(reg.get("definitely_not_here").is_err());
    let ids = reg.ids();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(ids.len(), sorted.len(), "в реестре есть повторяющиеся id");
}

#[test]
fn settings_default_roundtrip() {
    let dir = std::env::temp_dir().join(format!("gs-cfg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("s.json");
    let mut s = glitch_studio::core::config::Settings::default();
    s.library_folders.push(dir.clone());
    std::fs::write(&file, serde_json::to_string_pretty(&s).unwrap()).unwrap();

    let back: glitch_studio::core::config::Settings =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(back.library_folders, s.library_folders);
    assert_eq!(back.theme, s.theme);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn settings_recent_is_mru_capped() {
    let mut s = glitch_studio::core::config::Settings::default();
    for i in 0..30 {
        s.push_recent(std::path::PathBuf::from(format!("/tmp/f{i}.png")));
    }
    assert_eq!(s.recent_files.len(), 12);
    assert_eq!(s.recent_files[0], std::path::PathBuf::from("/tmp/f29.png"));
    s.push_recent(std::path::PathBuf::from("/tmp/f29.png"));
    assert_eq!(
        s.recent_files.len(),
        12,
        "повторный файл не должен дублироваться"
    );
}
