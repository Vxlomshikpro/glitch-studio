//! Декодирование видео в сырые RGBA-кадры через пайп ffmpeg.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};

use crate::core::error::{GlitchError, Result};
use crate::core::frame::Frame;

use super::probe::{MediaInfo, MediaKind};
use super::unique_temp_path;

/// Потоковый декодер: ffmpeg пишет rawvideo rgba в stdout, мы читаем по кадру.
///
/// stderr уводится в временный файл, а не в пайп: иначе на длинном видео
/// ffmpeg заполнит буфер пайпа и заблокируется на записи, а мы — на чтении кадров.
pub struct VideoDecoder {
    child: Child,
    stdout: BufReader<ChildStdout>,
    width: u32,
    height: u32,
    frame_bytes: usize,
    frames_read: u64,
    finished: bool,
    stderr_path: PathBuf,
}

impl VideoDecoder {
    /// Запускает ffmpeg, декодирующий файл в rawvideo rgba на stdout.
    pub fn open(path: &Path, info: &MediaInfo) -> Result<Self> {
        if info.kind != MediaKind::Video {
            return Err(GlitchError::other("VideoDecoder: файл не является видео"));
        }
        let frame_bytes = frame_bytes(info.width, info.height)
            .ok_or_else(|| GlitchError::other("VideoDecoder: некорректные размеры кадра"))?;

        let stderr_path = unique_temp_path("dec");
        let stderr_file = File::create(&stderr_path).map_err(|source| GlitchError::IoWrite {
            path: stderr_path.clone(),
            source,
        })?;

        let mut child = Command::new(crate::video::bundled::tool_path("ffmpeg"))
            .args(["-v", "error", "-noautorotate", "-i"])
            .arg(path)
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr_file))
            .spawn()
            .map_err(|source| spawn_error(path, source))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GlitchError::other("ffmpeg: не удалось получить stdout"))?;

        Ok(Self {
            child,
            stdout: BufReader::new(stdout),
            width: info.width,
            height: info.height,
            frame_bytes,
            frames_read: 0,
            finished: false,
            stderr_path,
        })
    }

    /// Следующий кадр в порядке потока или `None` на конце файла.
    /// Короткое чтение (обрезанный последний кадр) — это конец потока, не ошибка.
    pub fn read_frame(&mut self) -> Result<Option<Frame>> {
        if self.finished {
            return Ok(None);
        }
        let mut buf = vec![0u8; self.frame_bytes];
        match self.stdout.read_exact(&mut buf) {
            Ok(()) => {
                self.frames_read += 1;
                Frame::from_rgba(self.width, self.height, buf)
                    .map(Some)
                    .ok_or(GlitchError::BufferSize {
                        expected: self.frame_bytes,
                        got: 0,
                    })
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                self.finished = true;
                self.check_exit()?;
                Ok(None)
            }
            Err(source) => Err(GlitchError::IoRead {
                path: self.stderr_path.clone(),
                source,
            }),
        }
    }

    /// Если ffmpeg упал, не отдав ни одного кадра, — это ошибка, а не пустое видео.
    fn check_exit(&mut self) -> Result<()> {
        let status = self.child.wait().map_err(|source| GlitchError::IoRead {
            path: self.stderr_path.clone(),
            source,
        })?;
        if !status.success() && self.frames_read == 0 {
            let stderr = std::fs::read_to_string(&self.stderr_path).unwrap_or_default();
            return Err(GlitchError::Ffmpeg {
                code: status.code().unwrap_or(-1),
                stderr: stderr.trim().to_string(),
            });
        }
        Ok(())
    }

    /// Убивает дочерний ffmpeg. Вызывается автоматически из `Drop`.
    pub fn stop(&mut self) {
        if !self.finished {
            match self.child.try_wait() {
                Ok(Some(_)) => {}
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                }
            }
            self.finished = true;
        }
        let _ = std::fs::remove_file(&self.stderr_path);
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Один кадр из середины видео — для превью в библиотеке.
/// `None`, если файл не декодируется (битый, пустой, не видео).
pub fn grab_frame_at(path: &Path, seconds: f64) -> Result<Option<Frame>> {
    // Отсутствие ffmpeg — это не «битый файл», а сломанное окружение: отдаём ошибку.
    if !super::probe::ffmpeg_available() {
        return Err(GlitchError::FfmpegMissing);
    }
    let info = match super::probe::probe(path) {
        Ok(i) => i,
        Err(_) => return Ok(None),
    };
    if info.kind != MediaKind::Video {
        return Ok(None);
    }
    let bytes = match frame_bytes(info.width, info.height) {
        Some(n) => n,
        None => return Ok(None),
    };

    let t = if seconds.is_finite() && seconds > 0.0 {
        seconds
    } else {
        0.0
    };

    // `output()` читает stdout и stderr параллельно — дедлока на stderr не будет.
    let out = match Command::new(crate::video::bundled::tool_path("ffmpeg"))
        .args(["-v", "error", "-noautorotate", "-ss"])
        .arg(format!("{t:.3}"))
        .arg("-i")
        .arg(path)
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .output()
    {
        Ok(o) => o,
        Err(_) => return Ok(None),
    };

    if !out.status.success() || out.stdout.len() < bytes {
        return Ok(None);
    }
    let data = out.stdout[..bytes].to_vec();
    Ok(Frame::from_rgba(info.width, info.height, data))
}

fn frame_bytes(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 {
        return None;
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
}

fn spawn_error(path: &Path, source: std::io::Error) -> GlitchError {
    if source.kind() == std::io::ErrorKind::NotFound {
        GlitchError::FfmpegMissing
    } else {
        GlitchError::IoRead {
            path: path.to_path_buf(),
            source,
        }
    }
}
