//! Определение параметров медиафайла: картинка через `image`, видео через `ffprobe`.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::core::error::{GlitchError, Result};
use crate::core::media;

/// Что за файл перед нами.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

/// Всё, что нужно знать о файле до начала работы.
#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub kind: MediaKind,
    pub width: u32,
    pub height: u32,
    /// Кадров в секунду. Для картинок 0.
    pub fps: f64,
    pub duration_secs: f64,
    pub frame_count: u64,
    pub has_audio: bool,
    pub video_codec: String,
}

/// Есть ли в системе и ffmpeg, и ffprobe.
pub fn ffmpeg_available() -> bool {
    tool_ok("ffmpeg") && tool_ok("ffprobe")
}

fn tool_ok(name: &str) -> bool {
    Command::new(crate::video::bundled::tool_path(name))
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Определяет параметры файла. Картинки читаются через `image`, видео — через `ffprobe`.
pub fn probe(path: &Path) -> Result<MediaInfo> {
    let ext = media::ext_of(path).unwrap_or_default();
    if media::is_video_ext(&ext) {
        probe_video(path)
    } else {
        probe_image(path)
    }
}

fn probe_image(path: &Path) -> Result<MediaInfo> {
    let frame = media::load(path)?;
    Ok(MediaInfo {
        kind: MediaKind::Image,
        width: frame.width,
        height: frame.height,
        fps: 0.0,
        duration_secs: 0.0,
        frame_count: 1,
        has_audio: false,
        video_codec: String::new(),
    })
}

fn probe_video(path: &Path) -> Result<MediaInfo> {
    if !ffmpeg_available() {
        return Err(GlitchError::FfmpegMissing);
    }
    let out = Command::new(crate::video::bundled::tool_path("ffprobe"))
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|source| GlitchError::IoRead {
            path: path.to_path_buf(),
            source,
        })?;

    if !out.status.success() {
        return Err(GlitchError::other(format!(
            "ffprobe не смог прочитать {}",
            path.display()
        )));
    }

    let json: Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| GlitchError::other(format!("не удалось разобрать вывод ffprobe: {e}")))?;

    parse_probe(&json)
}

fn parse_probe(json: &Value) -> Result<MediaInfo> {
    let streams = json
        .get("streams")
        .and_then(Value::as_array)
        .ok_or_else(|| GlitchError::other("ffprobe: в ответе нет потоков"))?;

    let mut video: Option<&Value> = None;
    let mut has_audio = false;
    for s in streams {
        match s.get("codec_type").and_then(Value::as_str) {
            Some("video") => {
                if video.is_none() {
                    video = Some(s);
                }
            }
            Some("audio") => has_audio = true,
            _ => {}
        }
    }

    let v = video.ok_or_else(|| GlitchError::other("в файле нет видеопотока"))?;

    let width = v.get("width").and_then(num_u64).unwrap_or(0) as u32;
    let height = v.get("height").and_then(num_u64).unwrap_or(0) as u32;
    if width == 0 || height == 0 {
        return Err(GlitchError::other("ffprobe: нулевые размеры кадра"));
    }

    let video_codec = v
        .get("codec_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let fps = parse_fps(v);

    let duration_secs = json
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(num_f64)
        .or_else(|| v.get("duration").and_then(num_f64))
        .unwrap_or(0.0);

    let frame_count = v
        .get("nb_frames")
        .and_then(num_u64)
        .filter(|n| *n > 0)
        .unwrap_or_else(|| {
            if fps > 0.0 && duration_secs > 0.0 {
                (duration_secs * fps).round() as u64
            } else {
                0
            }
        });

    Ok(MediaInfo {
        kind: MediaKind::Video,
        width,
        height,
        fps,
        duration_secs,
        frame_count,
        has_audio,
        video_codec,
    })
}

/// `r_frame_rate` точнее для CFR, `avg_frame_rate` — для VFR. Берём первый ненулевой.
fn parse_fps(v: &Value) -> f64 {
    for key in ["r_frame_rate", "avg_frame_rate"] {
        if let Some(s) = v.get(key).and_then(Value::as_str) {
            if let Some(f) = parse_ratio(s) {
                if f > 0.0 {
                    return f;
                }
            }
        }
    }
    0.0
}

fn parse_ratio(s: &str) -> Option<f64> {
    let (n, d) = s.split_once('/')?;
    let n: f64 = n.trim().parse().ok()?;
    let d: f64 = d.trim().parse().ok()?;
    if d == 0.0 {
        None
    } else {
        Some(n / d)
    }
}

/// ffprobe отдаёт числа то строкой, то числом — принимаем оба варианта.
fn num_u64(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn num_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}
