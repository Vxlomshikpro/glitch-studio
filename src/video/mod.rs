//! Видео: probe, декодирование кадров, экспорт через ffmpeg.

pub mod bundled;
pub mod decoder;
pub mod encoder;
pub mod probe;

use std::path::Path;

use crate::core::error::{GlitchError, Result};
use crate::core::media;
use crate::core::pipeline::Pipeline;
use crate::core::registry::Registry;

pub use decoder::{grab_frame_at, VideoDecoder};
pub use encoder::{
    databend_mosh, export_video, Cancel, ExportOptions, ExportProgress, ExportStage,
};
pub use probe::{ffmpeg_available, probe, MediaInfo, MediaKind};

/// Уникальный путь во временном каталоге — для stderr дочерних ffmpeg-процессов.
///
/// stderr уводится в файл, а не в пайп: иначе на длинном видео буфер пайпа
/// заполнится и ffmpeg заблокируется, а вместе с ним и наш цикл чтения кадров.
pub(crate) fn unique_temp_path(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("glitch-{tag}-{pid}-{nanos}-{n}.log"))
}

/// Обработка одного видеофайла в CLI-режиме. Сигнатура зафиксирована: её зовёт `src/cli.rs`.
pub fn process_file(
    input: &Path,
    output: &Path,
    pipeline: &Pipeline,
    registry: &Registry,
    quality: u8,
) -> Result<()> {
    let info = probe::probe(input)?;
    if info.kind != MediaKind::Video {
        return Err(GlitchError::other("process_file: входной файл не видео"));
    }

    let opts = ExportOptions {
        fps: 0,
        quality,
        width: 0,
        height: 0,
        keep_audio: true,
        gif: media::ext_of(output).map(|e| e == "gif").unwrap_or(false),
        datamosh: false,
        history_frames: 8,
    };

    let cancel = Cancel::new();
    let progress = |_p: ExportProgress| {};
    export_video(input, output, pipeline, registry, &opts, &cancel, &progress)?;

    println!("готово: {} -> {}", input.display(), output.display());
    Ok(())
}
