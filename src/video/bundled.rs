//! Встроенный ffmpeg для Windows-сборки.
//!
//! На Windows exe лежит рядом с пользователем, и требовать отдельно ставить
//! ffmpeg — нечестно. Бинарники вшиты в exe и распаковываются в temp при
//! первом обращении; дальше используются оттуда. Если рядом с exe или в PATH
//! уже есть ffmpeg — берётся системный, встроенный не трогается.
//!
//! На других платформах всё как обычно: ищем ffmpeg в PATH.

use std::path::PathBuf;

#[cfg(target_os = "windows")]
const FFMPEG_EXE: &[u8] = include_bytes!("../../assets/binaries/ffmpeg.exe");

#[cfg(target_os = "windows")]
const FFPROBE_EXE: &[u8] = include_bytes!("../../assets/binaries/ffprobe.exe");

/// Путь, который можно отдать `Command::new`.
///
/// Не кэшируем результат между вызовами на уровень строки: проверка «лежит ли
/// уже файл» дешёвая, а состояние в `OnceLock` усложнило бы очистку temp.
pub fn tool_path(name: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        if let Some(p) = bundled_path(name) {
            return p.to_string_lossy().into_owned();
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = name;
    name.to_string()
}

#[cfg(target_os = "windows")]
fn bundled_path(name: &str) -> Option<PathBuf> {
    let payload: &[u8] = match name {
        "ffmpeg" => FFMPEG_EXE,
        "ffprobe" => FFPROBE_EXE,
        _ => return None,
    };

    let dir = std::env::temp_dir().join(format!("glitch-studio-{}", env!("CARGO_PKG_VERSION")));
    let dest = dir.join(format!("{name}.exe"));

    if std::fs::metadata(&dest).is_ok_and(|m| m.len() == payload.len() as u64) {
        return Some(dest);
    }

    // Пишем во временный файл и переименовываем: иначе параллельный запуск
    // двух копий exe может поймать наполовину записанный файл.
    let tmp = dir.join(format!("{name}-{}.part", std::process::id()));
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    if std::fs::write(&tmp, payload).is_err() {
        return None;
    }
    if std::fs::rename(&tmp, &dest).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    Some(dest)
}

/// Папка с распакованными бинарниками — её можно безопасно удалять.
pub fn unpack_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        Some(std::env::temp_dir().join(format!("glitch-studio-{}", env!("CARGO_PKG_VERSION"))))
    }
    #[cfg(not(target_os = "windows"))]
    None
}
