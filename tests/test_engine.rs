//! Движок фонового превью: задача должна выполниться и вернуть кадр.

use std::sync::Arc;
use std::time::{Duration, Instant};

use glitch_studio::core::frame::Frame;
use glitch_studio::core::pipeline::Pipeline;
use glitch_studio::core::registry::Registry;
use glitch_studio::ui::engine::PreviewEngine;

fn sample(w: u32, h: u32) -> Frame {
    let mut f = Frame::new(w, h);
    for y in 0..h {
        for x in 0..w {
            f.set(
                x as i64,
                y as i64,
                [(x % 256) as u8, (y % 256) as u8, 0, 255],
            );
        }
    }
    f
}

#[test]
fn engine_returns_a_frame_for_a_job() {
    let reg = Arc::new(Registry::new());
    let mut engine = PreviewEngine::new(Arc::clone(&reg), 4);

    let src = Arc::new(sample(120, 90));
    let pipeline = Pipeline::with_defaults(42, &reg);
    engine.request(src, &pipeline, Arc::new(Vec::new()));

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if engine.poll() {
            let out = engine
                .current
                .as_ref()
                .expect("current должен появиться после poll");
            assert_eq!((out.width, out.height), (120, 90));
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("движок не вернул кадр за 10 секунд");
}

#[test]
fn engine_result_is_reproducible() {
    let reg = Arc::new(Registry::new());
    let src = Arc::new(sample(64, 64));

    let run_once = || {
        let mut engine = PreviewEngine::new(Arc::clone(&reg), 4);
        let pipeline = Pipeline::with_defaults(7, &reg);
        engine.request(Arc::clone(&src), &pipeline, Arc::new(Vec::new()));
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if engine.poll() {
                return engine.current.clone().expect("кадр");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("движок не вернул кадр");
    };

    let a = run_once();
    let b = run_once();
    assert_eq!(a.data, b.data, "один seed обязан давать один результат");
}

#[test]
fn engine_is_replaced_not_accumulated() {
    let reg = Arc::new(Registry::new());
    let mut engine = PreviewEngine::new(Arc::clone(&reg), 4);
    let src = Arc::new(sample(48, 48));

    // Десять задач подряд — воркер считает только последнюю.
    for _ in 0..10 {
        let pipeline = Pipeline::with_defaults(1, &reg);
        engine.request(Arc::clone(&src), &pipeline, Arc::new(Vec::new()));
    }

    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if engine.poll() {
            let out = engine.current.clone().expect("кадр");
            assert_eq!((out.width, out.height), (48, 48));
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("движок не вернул кадр после серии задач");
}
