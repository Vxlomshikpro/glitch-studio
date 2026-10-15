//! Загрузка и сохранение изображений. Форматы: png, jpg, webp, gif.

use std::io::Cursor;
use std::path::Path;

use image::{DynamicImage, ImageFormat, ImageReader, RgbaImage};

use crate::core::error::{GlitchError, Result};
use crate::core::frame::Frame;

/// Расширения, которые программа открывает.
pub const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "bmp", "gif", "tga", "tif", "tiff",
];
pub const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v"];

/// Все поддерживаемые форматы, для диалогов и фильтров библиотеки.
pub fn is_media(path: &Path) -> bool {
    match ext_of(path).as_deref() {
        Some(e) => is_media_ext(e),
        None => false,
    }
}

pub fn is_video_ext(ext: &str) -> bool {
    VIDEO_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn is_image_ext(ext: &str) -> bool {
    IMAGE_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn is_media_ext(ext: &str) -> bool {
    is_image_ext(ext) || is_video_ext(ext)
}

pub fn ext_of(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
}

fn format_for(ext: &str) -> Option<ImageFormat> {
    match ext.to_ascii_lowercase().as_str() {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "webp" => Some(ImageFormat::WebP),
        "gif" => Some(ImageFormat::Gif),
        "bmp" => Some(ImageFormat::Bmp),
        "tga" => Some(ImageFormat::Tga),
        "tif" | "tiff" => Some(ImageFormat::Tiff),
        _ => None,
    }
}

pub fn load(path: &Path) -> Result<Frame> {
    let reader = ImageReader::open(path).map_err(|source| GlitchError::IoRead {
        path: path.to_path_buf(),
        source,
    })?;
    let reader = reader
        .with_guessed_format()
        .map_err(|source| GlitchError::IoRead {
            path: path.to_path_buf(),
            source,
        })?;
    let img = reader
        .decode()
        .map_err(|e| GlitchError::Decode(e.to_string()))?;
    Ok(from_dynamic(img))
}

/// Загрузка из памяти — используется для кадров видео и drag&drop из буфера.
pub fn load_from_bytes(bytes: &[u8]) -> Result<Frame> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| GlitchError::Decode(e.to_string()))?;
    let img = reader
        .decode()
        .map_err(|e| GlitchError::Decode(e.to_string()))?;
    Ok(from_dynamic(img))
}

pub fn from_dynamic(img: DynamicImage) -> Frame {
    let rgba: RgbaImage = img.to_rgba8();
    Frame {
        width: rgba.width(),
        height: rgba.height(),
        data: rgba.into_raw(),
    }
}

/// Сохраняет кадр. Формат берётся из расширения; `quality` (0..=100) влияет
/// только на jpeg и webp.
pub fn save(frame: &Frame, path: &Path, quality: u8) -> Result<()> {
    let ext = ext_of(path).unwrap_or_else(|| "png".to_string());
    let fmt = format_for(&ext).ok_or_else(|| {
        GlitchError::other(format!("формат .{ext} не поддерживается для сохранения"))
    })?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| GlitchError::IoWrite {
            path: path.to_path_buf(),
            source,
        })?;
    }

    let q = quality.clamp(1, 100);
    let result: std::result::Result<(), image::ImageError> = match fmt {
        ImageFormat::Jpeg => {
            let file = std::fs::File::create(path).map_err(|source| GlitchError::IoWrite {
                path: path.to_path_buf(),
                source,
            })?;
            let rgb = to_rgb(frame);
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(file, q);
            enc.encode(
                &rgb,
                frame.width,
                frame.height,
                image::ExtendedColorType::Rgb8,
            )
        }
        ImageFormat::WebP => {
            let file = std::fs::File::create(path).map_err(|source| GlitchError::IoWrite {
                path: path.to_path_buf(),
                source,
            })?;
            let _ = q;
            image::codecs::webp::WebPEncoder::new_lossless(file).encode(
                &frame.data,
                frame.width,
                frame.height,
                image::ExtendedColorType::Rgba8,
            )
        }
        other => image::save_buffer_with_format(
            path,
            &frame.data,
            frame.width,
            frame.height,
            image::ExtendedColorType::Rgba8,
            other,
        ),
    };

    result.map_err(|e| GlitchError::other(format!("не удалось сохранить: {e}")))
}

/// RGBA -> RGB для энкодеров, которые альфу не понимают (JPEG в image 0.25
/// принимает только L8 и Rgb8, Rgba8 возвращает Unsupported).
fn to_rgb(frame: &Frame) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(frame.pixel_count() * 3);
    for px in frame.data.chunks_exact(4) {
        rgb.push(px[0]);
        rgb.push(px[1]);
        rgb.push(px[2]);
    }
    rgb
}

/// Следующий свободный путь `имя_glitch_001.ext` в каталоге `dir`.
/// Номер растёт, существующие файлы не перезаписываются.
pub fn next_export_path(dir: &Path, stem: &str, ext: &str) -> std::path::PathBuf {
    for n in 1..100_000u32 {
        let candidate = dir.join(format!("{stem}_glitch_{n:03}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}_glitch_overflow.{ext}"))
}

/// Уменьшенная копия для быстрого превью.
pub fn downscale(frame: &Frame, max_w: u32, max_h: u32) -> Frame {
    if frame.width <= max_w && frame.height <= max_h {
        return frame.clone();
    }
    let scale = (max_w as f32 / frame.width as f32)
        .min(max_h as f32 / frame.height as f32)
        .min(1.0);
    let w = ((frame.width as f32 * scale).round() as u32).max(1);
    let h = ((frame.height as f32 * scale).round() as u32).max(1);
    resize_nearest(frame, w, h)
}

/// Ближайший сосед — быстро и достаточно для превью.
pub fn resize_nearest(frame: &Frame, w: u32, h: u32) -> Frame {
    let w = w.max(1) as usize;
    let h = h.max(1) as usize;
    let mut out = Frame::new(w as u32, h as u32);
    let src_w = frame.width.max(1) as usize;
    let src_h = frame.height.max(1) as usize;

    for y in 0..h {
        let sy = (y * src_h / h).min(src_h - 1);
        let src_row = &frame.data[sy * frame.stride()..(sy + 1) * frame.stride()];
        let dst_row_start = y * w * 4;
        for x in 0..w {
            let sx = (x * src_w / w).min(src_w - 1) * 4;
            let d = dst_row_start + x * 4;
            out.data[d..d + 4].copy_from_slice(&src_row[sx..sx + 4]);
        }
    }
    out
}
