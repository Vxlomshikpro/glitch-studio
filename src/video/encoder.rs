//! Кодирование обработанных кадров обратно в видео через ffmpeg, с сохранением звука.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::core::error::{GlitchError, Result};
use crate::core::frame::Frame;
use crate::core::media;
use crate::core::pipeline::Pipeline;
use crate::core::registry::Registry;

use super::decoder::VideoDecoder;
use super::probe::{self, MediaKind};
use super::unique_temp_path;

/// Прогресс экспорта для UI.
#[derive(Debug, Clone, Copy)]
pub struct ExportProgress {
    pub frame: u64,
    pub total: u64,
    pub stage: ExportStage,
}

/// Стадия экспорта. Идёт строго по порядку Decoding -> Processing -> Encoding -> Done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportStage {
    Decoding,
    Processing,
    Encoding,
    Done,
}

/// Кооперативная отмена экспорта. UI дёргает `cancel()`, цикл проверяет флаг.
/// Клонируется, чтобы UI мог держать копию, а рабочий поток — свою.
#[derive(Clone)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl Default for Cancel {
    fn default() -> Self {
        Self::new()
    }
}

/// Параметры экспорта видео.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub fps: u32,
    /// 1..=100. Переводится в CRF для libx264.
    pub quality: u8,
    /// 0 = оставить исходный размер. Масштаб применяется, только если заданы оба.
    pub width: u32,
    pub height: u32,
    pub keep_audio: bool,
    /// true = палитровый GIF через ffmpeg.
    pub gif: bool,
    pub datamosh: bool,
    /// Сколько прошлых кадров отдавать в `Pipeline::apply`.
    pub history_frames: usize,
}

/// Прогоняет видео через пайплайн и пишет результат.
///
/// Кадры декодируются потоково, обрабатываются и сразу уходят в stdin энкодера —
/// в памяти держится только кольцо истории (`history_frames`), а не всё видео.
pub fn export_video(
    input: &Path,
    output: &Path,
    pipeline: &Pipeline,
    registry: &Registry,
    opts: &ExportOptions,
    cancel: &Cancel,
    on_progress: &(dyn Fn(ExportProgress) + Send + Sync),
) -> Result<()> {
    if !probe::ffmpeg_available() {
        return Err(GlitchError::FfmpegMissing);
    }
    let info = probe::probe(input)?;
    if info.kind != MediaKind::Video {
        return Err(GlitchError::other("экспорт видео: входной файл не видео"));
    }

    let (out_w, out_h) = resolve_dims(info.width, info.height, opts.width, opts.height);
    let fps = resolve_fps(opts.fps, info.fps);
    let total = if info.frame_count > 0 {
        info.frame_count
    } else if info.duration_secs > 0.0 {
        (info.duration_secs * fps as f64).round() as u64
    } else {
        0
    };

    create_parent(output)?;

    let crf = crf_from_quality(opts.quality);
    let keep_audio = opts.keep_audio && !opts.gif;
    let mut enc = Encoder::spawn(
        input, output, out_w, out_h, fps, crf, keep_audio, false, opts.gif,
    )?;

    let mut dec = match VideoDecoder::open(input, &info) {
        Ok(d) => d,
        Err(e) => {
            enc.abort();
            return Err(e);
        }
    };

    let hist = opts.history_frames.min(8);
    let mut ring: Vec<std::sync::Arc<Frame>> = Vec::with_capacity(hist);
    let mut frame_idx: u64 = 0;

    on_progress(ExportProgress {
        frame: 0,
        total,
        stage: ExportStage::Decoding,
    });

    loop {
        if cancel.is_cancelled() {
            dec.stop();
            enc.abort();
            return Err(GlitchError::other("отменено"));
        }

        let frame = match dec.read_frame() {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                dec.stop();
                enc.abort();
                return Err(e);
            }
        };

        on_progress(ExportProgress {
            frame: frame_idx,
            total,
            stage: ExportStage::Processing,
        });

        let work = if frame.width != out_w || frame.height != out_h {
            media::resize_nearest(&frame, out_w, out_h)
        } else {
            frame
        };

        // Ошибка одного эффекта не должна ронять весь экспорт — кадр идёт как есть.
        let mut out_frame = pipeline
            .apply(&work, registry, &ring)
            .unwrap_or_else(|_| work.clone());

        if opts.datamosh {
            databend_shift(&mut out_frame, frame_idx, opts.quality);
        }

        if hist > 0 {
            ring.push(std::sync::Arc::new(work));
            if ring.len() > hist {
                ring.remove(0);
            }
        }

        on_progress(ExportProgress {
            frame: frame_idx,
            total,
            stage: ExportStage::Encoding,
        });

        if let Err(e) = enc.write_frame(&out_frame) {
            dec.stop();
            enc.abort();
            return Err(e);
        }
        frame_idx += 1;
    }

    dec.stop();
    enc.finish()?;

    on_progress(ExportProgress {
        frame: frame_idx,
        total,
        stage: ExportStage::Done,
    });
    Ok(())
}

/// Databend: сдвигает байты каждого кадра и перекодирует, звук копируется как есть.
///
/// Классический рецепт через `-bsf:v noise=alls=...` на ffmpeg 8.1.1 не работает
/// (у фильтра `noise` другие опции), поэтому используется честный байтовый сдвиг —
/// ровно то, что делает databend в hex-редакторе.
pub fn databend_mosh(input: &Path, output: &Path, iterations: u32, quality: u8) -> Result<()> {
    if !probe::ffmpeg_available() {
        return Err(GlitchError::FfmpegMissing);
    }
    let info = probe::probe(input)?;
    if info.kind != MediaKind::Video {
        return Err(GlitchError::other("databend: входной файл не видео"));
    }

    create_parent(output)?;

    let fps = resolve_fps(0, info.fps);
    let crf = crf_from_quality(quality);
    let mut enc = Encoder::spawn(
        input,
        output,
        info.width,
        info.height,
        fps,
        crf,
        true,
        true,
        false,
    )?;

    let mut dec = match VideoDecoder::open(input, &info) {
        Ok(d) => d,
        Err(e) => {
            enc.abort();
            return Err(e);
        }
    };

    let iters = iterations.max(1) as usize;
    let mut idx: u64 = 0;
    loop {
        let mut frame = match dec.read_frame() {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                dec.stop();
                enc.abort();
                return Err(e);
            }
        };
        databend_shift_iters(&mut frame, idx, iters);
        if let Err(e) = enc.write_frame(&frame) {
            dec.stop();
            enc.abort();
            return Err(e);
        }
        idx += 1;
    }

    dec.stop();
    enc.finish()?;
    Ok(())
}

/// Обёртка над дочерним ffmpeg-энкодером: пишем кадры в stdin, читаем stderr из файла.
struct Encoder {
    child: Child,
    stdin: Option<ChildStdin>,
    stderr_path: PathBuf,
    output: PathBuf,
    frame_bytes: usize,
    done: bool,
}

impl Encoder {
    #[allow(clippy::too_many_arguments)]
    fn spawn(
        input: &Path,
        output: &Path,
        w: u32,
        h: u32,
        fps: u32,
        crf: u32,
        keep_audio: bool,
        audio_copy: bool,
        gif: bool,
    ) -> Result<Self> {
        let frame_bytes = (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| GlitchError::other("энкодер: слишком большой кадр"))?;

        let stderr_path = unique_temp_path("enc");
        let stderr_file = File::create(&stderr_path).map_err(|source| GlitchError::IoWrite {
            path: stderr_path.clone(),
            source,
        })?;

        let mut cmd = Command::new(crate::video::bundled::tool_path("ffmpeg"));
        cmd.args([
            "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s",
        ])
        .arg(format!("{w}x{h}"))
        .arg("-r")
        .arg(fps.to_string())
        .args(["-i", "pipe:0"]);

        if gif {
            let vf = format!(
                "fps={fps},scale={w}:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse"
            );
            cmd.arg("-vf").arg(vf).args(["-loop", "0"]);
        } else {
            if keep_audio {
                // `-map 1:a?` с вопросиком: если звуковой дорожки нет, ffmpeg не падает.
                cmd.arg("-i").arg(input);
                cmd.args(["-map", "0:v", "-map", "1:a?"]);
            } else {
                cmd.args(["-map", "0:v"]);
            }
            cmd.args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf"])
                .arg(crf.to_string())
                .args(["-preset", "veryfast"]);
            if keep_audio {
                if audio_copy {
                    cmd.args(["-c:a", "copy"]);
                }
                cmd.arg("-shortest");
            }
        }

        cmd.arg(output)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_file));

        let mut child = cmd.spawn().map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                GlitchError::FfmpegMissing
            } else {
                GlitchError::IoWrite {
                    path: output.to_path_buf(),
                    source,
                }
            }
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| GlitchError::other("ffmpeg: не удалось получить stdin"))?;

        Ok(Self {
            child,
            stdin: Some(stdin),
            stderr_path,
            output: output.to_path_buf(),
            frame_bytes,
            done: false,
        })
    }

    fn write_frame(&mut self, frame: &Frame) -> Result<()> {
        if frame.data.len() != self.frame_bytes {
            return Err(GlitchError::BufferSize {
                expected: self.frame_bytes,
                got: frame.data.len(),
            });
        }
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| GlitchError::other("ffmpeg: stdin уже закрыт"))?;
        stdin
            .write_all(&frame.data)
            .map_err(|source| GlitchError::IoWrite {
                path: self.output.clone(),
                source,
            })
    }

    /// Закрывает stdin, ждёт ffmpeg и проверяет результат.
    fn finish(mut self) -> Result<()> {
        self.done = true;
        drop(self.stdin.take());

        let status = self.child.wait().map_err(|source| GlitchError::IoWrite {
            path: self.output.clone(),
            source,
        })?;
        let stderr = std::fs::read_to_string(&self.stderr_path).unwrap_or_default();
        let _ = std::fs::remove_file(&self.stderr_path);

        if !status.success() {
            let _ = std::fs::remove_file(&self.output);
            return Err(GlitchError::Ffmpeg {
                code: status.code().unwrap_or(-1),
                stderr: stderr.trim().to_string(),
            });
        }
        if !self.output.exists() {
            return Err(GlitchError::other(format!(
                "ffmpeg не создал файл {}",
                self.output.display()
            )));
        }
        Ok(())
    }

    /// Аварийно убивает ffmpeg и удаляет недописанный файл.
    fn abort(&mut self) {
        self.done = true;
        drop(self.stdin.take());
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
        let _ = std::fs::remove_file(&self.stderr_path);
        let _ = std::fs::remove_file(&self.output);
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        if !self.done {
            self.abort();
        }
    }
}

fn resolve_dims(src_w: u32, src_h: u32, w: u32, h: u32) -> (u32, u32) {
    if w > 0 && h > 0 {
        (w, h)
    } else {
        (src_w.max(1), src_h.max(1))
    }
}

fn resolve_fps(opt: u32, src: f64) -> u32 {
    if opt > 0 {
        opt
    } else if src > 0.0 && src.is_finite() {
        (src.round() as u32).max(1)
    } else {
        25
    }
}

/// quality 1..=100 -> CRF 51..=0 (выше качество — ниже CRF).
fn crf_from_quality(quality: u8) -> u32 {
    let q = quality.clamp(1, 100) as f32;
    (((100.0 - q) * 51.0 / 100.0).round() as u32).min(51)
}

fn create_parent(output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|source| GlitchError::IoWrite {
                path: output.to_path_buf(),
                source,
            })?;
        }
    }
    Ok(())
}

/// Циклический сдвиг байтов кадра — сдвиг не кратен 4, поэтому каналы разъезжаются.
fn databend_shift(frame: &mut Frame, frame_idx: u64, quality: u8) {
    let len = frame.data.len();
    if len == 0 {
        return;
    }
    let amount = ((quality as usize).wrapping_mul(7))
        .wrapping_add((frame_idx as usize).wrapping_mul(13))
        % len;
    if amount != 0 {
        frame.data.rotate_left(amount);
    }
}

fn databend_shift_iters(frame: &mut Frame, frame_idx: u64, iterations: usize) {
    let len = frame.data.len();
    if len == 0 {
        return;
    }
    let amount = iterations
        .wrapping_mul(31)
        .wrapping_add((frame_idx as usize).wrapping_mul(17))
        % len;
    if amount != 0 {
        frame.data.rotate_left(amount);
    }
}
