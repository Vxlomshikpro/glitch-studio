//! Ошибки приложения. Ни одна из них не приводит к панике — всё показывается в UI.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GlitchError {
    #[error("Не удалось прочитать файл {path}: {source}")]
    IoRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Не удалось записать файл {path}: {source}")]
    IoWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Не удалось разобрать изображение: {0}")]
    Decode(String),

    #[error("Размер буфера не совпадает: ожидалось {expected} байт, получено {got}")]
    BufferSize { expected: usize, got: usize },

    #[error("Эффект {id} вернул ошибку: {reason}")]
    Effect { id: String, reason: String },

    #[error("ffmpeg завершился с кодом {code}: {stderr}")]
    Ffmpeg { code: i32, stderr: String },

    #[error("ffmpeg не найден в системе. Установи пакет ffmpeg через apt.")]
    FfmpegMissing,

    #[error("Неизвестный пресет: {0}")]
    UnknownPreset(String),

    #[error("Эффект «{0}» не найден в реестре")]
    UnknownEffect(String),

    #[error("{0}")]
    Other(String),
}

impl GlitchError {
    pub fn effect(id: &str, reason: impl Into<String>) -> Self {
        Self::Effect {
            id: id.to_string(),
            reason: reason.into(),
        }
    }

    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, GlitchError>;
