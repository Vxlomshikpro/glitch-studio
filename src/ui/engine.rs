//! Фоновое считание превью. UI не должен ждать ни одного эффекта.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use rayon::ThreadPool;

use crate::core::frame::Frame;
use crate::core::media;
use crate::core::pipeline::Pipeline;
use crate::core::registry::Registry;

type Job = (
    Arc<Frame>,
    Arc<Pipeline>,
    Arc<Vec<Arc<Frame>>>,
);

/// Слот для следующей задачи. Пока воркер считает, новые кадры перезаписывают
/// этот слот: очередь не растёт, считается всегда самый свежий кадр.
type Pending = Arc<Mutex<Option<Job>>>;


/// Хранилище завершённых результатов: воркер кладёт сюда, UI забирает.
type Outbox = Arc<Mutex<Vec<(u64, Frame)>>>;

/// Считает превью в отдельном пуле rayon и отдаёт результат по требованию.
///
/// Одновременно живёт только одна задача: ползунок дёрнули десять раз подряд —
/// десять промежуточных результатов не считаются, считается только последний.
pub struct PreviewEngine {
    tx: Sender<Job>,
    rx: Receiver<u64>,
    outbox: Outbox,
    pending: Pending,
    generation: u64,
    /// Результат последнего завершённого расчёта — его рисуем.
    pub current: Option<Frame>,
    /// Хэш пайплайна, для которого `current` актуален.
    pub current_hash: Option<u64>,
    /// Сколько кадров истории требуется пайплайну.
    pub history_len: usize,
    pub busy: bool,
    pool: ThreadPool,
    last_error: Option<String>,
}

impl PreviewEngine {
    pub fn new(registry: Arc<Registry>, history_len: usize) -> Self {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .thread_name(|i| format!("gs-preview-{i}"))
            .build()
            .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().expect("rayon pool"));

        let (tx_out, rx_out) = std::sync::mpsc::channel::<u64>();
        let (tx, rx_in): (Sender<Job>, Receiver<Job>) = std::sync::mpsc::channel();
        let worker_tx_job = tx.clone();
        let outbox: Outbox = Arc::new(Mutex::new(Vec::new()));
        let pending: Pending = Arc::new(Mutex::new(None));

        let worker_registry = Arc::clone(&registry);
        let worker_outbox = Arc::clone(&outbox);
        let worker_pending = Arc::clone(&pending);
        let worker_tx = tx_out.clone();

        std::thread::Builder::new()
            .name("gs-preview-dispatch".to_string())
            .spawn(move || {
                for (source, pipeline, prev) in rx_in {
                    let hash = pipeline.content_hash();
                    let frame = pipeline
                        .apply(&source, &worker_registry, &prev)
                        .unwrap_or_else(|e| {
                            tracing::warn!("превью не посчиталось: {e}");
                            source.as_ref().clone()
                        });
                    if let Ok(mut b) = worker_outbox.lock() {
                        b.clear();
                        b.push((hash, frame));
                    }
                    let _ = worker_tx.send(hash);

                    // Пока считался этот кадр, мог прийти более свежий: берём его,
                    // вместо того чтобы копить очередь кадров.
                    let next = worker_pending.lock().ok().and_then(|mut p| p.take());
                    if let Some(next) = next {
                        let _ = worker_tx_job.send(next);
                    }
                }
            })
            .ok();

        Self {
            tx,
            rx: rx_out,
            outbox,
            pending,
            generation: 0,
            current: None,
            current_hash: None,
            history_len,
            busy: false,
            pool,
            last_error: None,
        }
    }

    /// Ставит задачу. `source` — уже уменьшенная копия оригинала.
    pub fn request(
        &mut self,
        source: Arc<Frame>,
        pipeline: &Pipeline,
        prev: Arc<Vec<Arc<Frame>>>,
    ) {
        self.generation = self.generation.wrapping_add(1);
        self.busy = true;
        let job = (source, Arc::new(pipeline.clone()), prev);

        // Если воркер сейчас считает — не копим очередь, а перезаписываем слот:
        // при перетаскивании ползунка и при воспроизведении видео это ключевое.
        if let Ok(mut slot) = self.pending.lock() {
            if slot.is_some() {
                *slot = Some(job);
                return;
            }
        }
        if let Ok(mut b) = self.outbox.lock() {
            b.clear();
        }
        let _ = self.tx.send(job);
    }

    /// Забирает готовый кадр, если он есть. Вызывать каждый кадр UI.
    pub fn poll(&mut self) -> bool {
        let mut got = false;
        while self.rx.try_recv().is_ok() {
            if let Ok(mut b) = self.outbox.lock() {
                if let Some((h, frame)) = b.pop() {
                    self.current = Some(frame);
                    self.current_hash = Some(h);
                    got = true;
                }
                b.clear();
            }
        }
        // Подстраховка на случай, если ответное письмо ещё в пути, а кадр уже есть.
        if !got {
            if let Ok(mut b) = self.outbox.lock() {
                if let Some((h, frame)) = b.pop() {
                    self.current = Some(frame);
                    self.current_hash = Some(h);
                    got = true;
                }
                b.clear();
            }
        }
        if got {
            self.busy = false;
        }
        got
    }

    pub fn error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Сбрасывает кэш — при смене файла или seed.
    pub fn invalidate(&mut self) {
        self.current = None;
        self.current_hash = None;
        if let Ok(mut b) = self.outbox.lock() {
            b.clear();
        }
    }

    pub fn pool(&self) -> &ThreadPool {
        &self.pool
    }
}

impl Drop for PreviewEngine {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.pending.lock() {
            slot.take();
        }
        // Канал закрывается вместе с Sender: воркер выходит из for и завершается.
    }
}

/// Уменьшенная копия источника под превью.
pub fn preview_source(source: &Frame, max_w: u32) -> Frame {
    media::downscale(source, max_w, u32::MAX / 2)
}
