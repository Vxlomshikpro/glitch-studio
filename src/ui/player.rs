//! Плеер видео: позиция, play/pause, покадровый шаг, история кадров для эффектов.

use std::path::PathBuf;
use std::sync::Arc;

use crate::core::frame::Frame;
use crate::core::media;
use crate::video::{grab_frame_at, MediaInfo, VideoDecoder};

/// Сколько кадров держим для temporal-эффектов.
pub const HISTORY: usize = 8;

pub struct Player {
    pub path: Option<PathBuf>,
    pub info: Option<MediaInfo>,
    decoder: Option<VideoDecoder>,
    /// Номер текущего кадра.
    pub frame_no: u64,
    pub playing: bool,
    /// Кадры, попадающие в превью, уменьшены до этой ширины.
    pub preview_width: u32,
    current: Option<Frame>,
    full: Option<Frame>,
    history: Vec<Arc<Frame>>,
    /// Вре��я последнего кадра в секундах — для шага назад.
    last_pts: f64,
    queued: bool,
    /// Декодер стоит в начале, а показываем кадр середины: перемотать перед play.
    needs_seek: bool,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            path: None,
            info: None,
            decoder: None,
            frame_no: 0,
            playing: false,
            preview_width: 960,
            current: None,
            full: None,
            history: Vec::new(),
            last_pts: 0.0,
            queued: false,
            needs_seek: false,
        }
    }
}

impl Player {
    pub fn is_open(&self) -> bool {
        self.decoder.is_some()
    }

    pub fn is_video(&self) -> bool {
        matches!(
            self.path.as_deref().and_then(media::ext_of),
            Some(ref e) if media::is_video_ext(e)
        )
    }

    pub fn duration(&self) -> f64 {
        self.info.as_ref().map(|i| i.duration_secs).unwrap_or(0.0)
    }

    pub fn fps(&self) -> f64 {
        let f = self.info.as_ref().map(|i| i.fps).unwrap_or(25.0);
        if f.is_finite() && f > 0.0 {
            f
        } else {
            25.0
        }
    }

    pub fn total_frames(&self) -> u64 {
        self.info
            .as_ref()
            .map(|i| {
                if i.frame_count > 0 {
                    i.frame_count
                } else {
                    (self.duration() * self.fps()).max(0.0) as u64
                }
            })
            .unwrap_or(0)
    }

    pub fn current_seconds(&self) -> f64 {
        self.last_pts.max(0.0)
    }

    /// Кадр, готовый к отрисовке (уменьшенный, с эффектами накладывает вызывающий).
    pub fn frame(&self) -> Option<&Frame> {
        self.current.as_ref()
    }

    /// Кадр в полном разрешении — для экспорта.
    pub fn full_frame(&self) -> Option<&Frame> {
        self.full.as_ref()
    }

    /// Копия только указателей: кадры не клонируются, поэтому видео не дёргается.
    pub fn history(&self) -> Arc<Vec<Arc<Frame>>> {
        Arc::new(self.history.clone())
    }

    pub fn open(&mut self, path: PathBuf, info: Option<MediaInfo>) -> Result<(), String> {
        self.stop();
        self.path = Some(path.clone());
        self.info = info;
        self.frame_no = 0;
        self.history.clear();

        if self.is_video() {
            let info = match self.info.clone() {
                Some(i) => i,
                None => crate::video::probe(&path).map_err(|e| e.to_string())?,
            };
            let total = if info.frame_count > 0 {
                info.frame_count
            } else {
                (info.duration_secs * self.fps()).max(0.0) as u64
            };

            // Показываем кадр из середины ролика: первый кадр у видео почти всегда
            // чёрный или однотонный (заставка, fade-in), и по нему нельзя понять,
            // что вообще в ролике. Кадр берём отдельным быстрым запросом к ffmpeg,
            // а не перемоткой — иначе открытие длинного видеика займёт десятки секунд.
            let mid_frame_no = (total / 2).clamp(0, total.saturating_sub(1));
            let mid_secs = info.duration_secs * 0.5;
            let poster = if total > 2 && mid_secs > 0.0 {
                grab_frame_at(&path, mid_secs).ok().flatten()
            } else {
                None
            };

            let mut decoder = VideoDecoder::open(&path, &info).map_err(|e| e.to_string())?;
            let had_poster = poster.is_some();
            let full = match poster {
                Some(f) => {
                    decoder.read_frame().ok().flatten();
                    Some(f)
                }
                None => decoder.read_frame().ok().flatten(),
            };
            if let Some(f) = full {
                self.full = Some(f.clone());
                self.current = Some(media::downscale(&f, self.preview_width, u32::MAX));
                self.push_history(&f);
                self.frame_no = mid_frame_no;
                self.last_pts = mid_frame_no as f64 / self.fps();
                // Декодер стоит в начале, реальная перемотка — при первом воспроизведении.
                self.needs_seek = had_poster && mid_frame_no > 0;
            }
            self.decoder = Some(decoder);
            self.queued = true;
        }
        Ok(())
    }

    fn push_history(&mut self, frame: &Frame) {
        if let Some(last) = self.history.last() {
            if last.width == frame.width && last.height == frame.height {
                // Пиксели те же — Arc без копии буферов.
                self.history.push(Arc::new(frame.clone()));
            } else {
                self.history.clear();
                self.history.push(Arc::new(frame.clone()));
            }
        } else {
            self.history.push(Arc::new(frame.clone()));
        }
        if self.history.len() > HISTORY {
            let excess = self.history.len() - HISTORY;
            self.history.drain(0..excess);
        }
    }

    /// Продвинуть на кадр. `n` — сколько кадров.
    pub fn step(&mut self, n: i64) {
        if self.decoder.is_none() {
            return;
        }
        let mut target = self.frame_no as i64 + n;
        if target < 0 {
            target = 0;
        }
        if target as u64 >= self.total_frames() && self.total_frames() > 0 {
            target = self.total_frames() as i64 - 1;
        }
        self.seek_to(target as u64);
    }

    pub fn seek_to(&mut self, frame_no: u64) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let Ok(sec) = self.seek_to_seconds() else {
            return;
        };
        let _ = sec;
        self.stop_decoder_only();
        let Some(info) = self.info.clone() else {
            return;
        };
        let mut decoder = match VideoDecoder::open(&path, &info) {
            Ok(d) => d,
            Err(_) => return,
        };
        // Перематываем кадры до нужного — ffmpeg seek по кадрам не умеет.
        let mut n = 0u64;
        while n < frame_no {
            match decoder.read_frame() {
                Ok(Some(_)) => n += 1,
                Ok(None) => break,
                Err(_) => return,
            }
        }
        match decoder.read_frame() {
            Ok(Some(f)) => {
                self.push_history(&f);
                self.full = Some(f.clone());
                self.current = Some(media::downscale(&f, self.preview_width, u32::MAX));
                self.frame_no = frame_no;
                self.last_pts = frame_no as f64 / self.fps();
                self.decoder = Some(decoder);
                self.queued = true;
            }
            _ => {
                self.stop_decoder_only();
            }
        }
    }

    fn seek_to_seconds(&self) -> Result<f64, ()> {
        Ok(self.frame_no as f64 / self.fps())
    }

    fn stop_decoder_only(&mut self) {
        if let Some(mut d) = self.decoder.take() {
            d.stop();
        }
    }

    pub fn stop(&mut self) {
        self.stop_decoder_only();
        self.playing = false;
        self.path = None;
        self.info = None;
        self.full = None;
        self.current = None;
        self.history.clear();
        self.frame_no = 0;
        self.needs_seek = false;
    }

    /// Вызывается каждый кадр UI: тянет следующий кадр, если играем.
    /// Возвращает true, если появился новый кадр — пора пересчитать превью.
    pub fn tick(&mut self) -> bool {
        if !self.playing || self.decoder.is_none() {
            return false;
        }
        if self.needs_seek {
            self.needs_seek = false;
            let target = self.frame_no;
            self.seek_to(target);
            return true;
        }
        if self.queued {
            self.queued = false;
        }
        match self.decoder.as_mut().map(|d| d.read_frame()) {
            Some(Ok(Some(f))) => {
                self.push_history(&f);
                self.full = Some(f.clone());
                self.current = Some(media::downscale(&f, self.preview_width, u32::MAX));
                self.frame_no += 1;
                self.last_pts = self.frame_no as f64 / self.fps();
                if self.frame_no >= self.total_frames() && self.total_frames() > 0 {
                    self.playing = false;
                }
                true
            }
            Some(Ok(None)) => {
                self.playing = false;
                false
            }
            _ => {
                self.playing = false;
                false
            }
        }
    }

    /// Один кадр из середины ролика — для превью в библиотеке.
    pub fn preview_frame(path: &std::path::Path) -> Option<Frame> {
        let info = crate::video::probe(path).ok()?;
        if info.kind == crate::video::MediaKind::Image {
            return media::load(path).ok();
        }
        grab_frame_at(path, info.duration_secs * 0.5).ok().flatten()
    }
}
