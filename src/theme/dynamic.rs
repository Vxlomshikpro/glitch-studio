//! Dynamic palette sources: From Image, From Wallpaper, Custom Color.
//!
//! The dominant-color extraction is a real (if simple) histogram: pixels are
//! bucketed by hue, near-black/near-white/grey pixels are discarded, and the
//! most populated bucket's average RGB becomes the seed. That seed is then
//! expanded by [`super::palette`] into a full Material You palette.

use std::path::{Path, PathBuf};

use crate::core::Frame;

use super::palette::Palette;

/// A palette plus a human-readable label describing where it came from.
#[derive(Debug, Clone)]
pub struct DynamicPalette {
    pub palette: Palette,
    pub source_label: String,
}

/// Palette derived from raw RGBA8 pixel data.
pub fn extract_from_pixels(pixels: &[u8], light: bool) -> Palette {
    Palette::from_pixels(pixels, light)
}

/// Palette derived from a decoded [`Frame`]. `None` for a 0-pixel frame.
pub fn extract_from_image(frame: &Frame, light: bool) -> Option<Palette> {
    if frame.pixel_count() == 0 || frame.data.is_empty() {
        return None;
    }
    Some(Palette::from_pixels(&frame.data, light))
}

/// Palette derived from the current desktop wallpaper.
///
/// Tries, in order: Hyprland configs (`hyprpaper`/`swww`), then cached lock /
/// swww backgrounds. Returns `None` if nothing usable is found. Never panics
/// on missing files.
pub fn extract_from_wallpaper(light: bool) -> Option<Palette> {
    let path = wallpaper_path()?;
    let img = image::ImageReader::open(&path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let rgba = img.to_rgba8();
    let raw = rgba.as_raw();
    if raw.is_empty() {
        return None;
    }
    Some(Palette::from_pixels(raw, light))
}

/// Picks the dominant hue from RGBA8 pixels and returns its average RGB.
///
/// Skips transparent, near-black, near-white and near-grey pixels so the seed
/// reflects the image's actual color identity. `None` if nothing qualifies.
pub(crate) fn dominant_seed(pixels: &[u8]) -> Option<(u8, u8, u8)> {
    const BUCKETS: usize = 24;
    let mut count = [0u32; BUCKETS];
    let mut sum = [[0u64; 3]; BUCKETS];
    let mut total = 0u32;

    for px in pixels.chunks_exact(4) {
        if px[3] < 128 {
            continue;
        }
        let (r, g, b) = (px[0], px[1], px[2]);
        let (h, s, v) = rgb_to_hsv(r, g, b);
        if !(0.12..=0.95).contains(&v) || s < 0.12 {
            continue;
        }
        let idx = (((h / 360.0) * BUCKETS as f32) as usize).min(BUCKETS - 1);
        count[idx] += 1;
        sum[idx][0] += u64::from(r);
        sum[idx][1] += u64::from(g);
        sum[idx][2] += u64::from(b);
        total += 1;
    }

    if total == 0 {
        return None;
    }
    let best = count.iter().enumerate().max_by_key(|(_, c)| **c)?.0;
    let n = u64::from(count[best].max(1));
    Some((
        (sum[best][0] / n) as u8,
        (sum[best][1] / n) as u8,
        (sum[best][2] / n) as u8,
    ))
}

/// RGB (0..255) -> HSV with hue in `0..360`, saturation/value in `0..1`.
fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let rf = f32::from(r) / 255.0;
    let gf = f32::from(g) / 255.0;
    let bf = f32::from(b) / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;

    let h = if delta <= f32::EPSILON {
        0.0
    } else if max <= rf {
        60.0 * (((gf - bf) / delta) % 6.0)
    } else if max <= gf {
        60.0 * (((bf - rf) / delta) + 2.0)
    } else {
        60.0 * (((rf - gf) / delta) + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max <= f32::EPSILON {
        0.0
    } else {
        delta / max
    };
    (h, s, max)
}

/// Resolves a wallpaper image path from Hyprland configs or caches.
fn wallpaper_path() -> Option<PathBuf> {
    let home = home_dir()?;

    // 1. Hyprland configs (hyprpaper / swww).
    if let Some(p) = scan_conf_dir(&home.join(".config/hypr")) {
        return Some(p);
    }

    // 2. Cached lock / swww backgrounds.
    for cand in [".cache/hyprlock/.lock.img", ".cache/swww/background"] {
        let p = home.join(cand);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

/// Scans `*.conf` files in `dir` for a wallpaper image path.
fn scan_conf_dir(dir: &Path) -> Option<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .map(|x| x.eq_ignore_ascii_case("conf"))
                .unwrap_or(false)
        })
        .collect();
    entries.sort();

    for p in entries {
        if let Ok(text) = std::fs::read_to_string(&p) {
            if let Some(img) = extract_path_from_conf(&text) {
                return Some(img);
            }
        }
    }
    None
}

/// Pulls the first existing image path out of a Hyprland-style config.
fn extract_path_from_conf(text: &str) -> Option<PathBuf> {
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        let interesting = lower.contains("wallpaper")
            || lower.contains("preload")
            || lower.contains("path")
            || lower.contains("swww")
            || lower.contains("img");
        if !interesting {
            continue;
        }

        let value = match line.split_once('=') {
            Some((_, v)) => v,
            None => line
                .split_once(char::is_whitespace)
                .map(|(_, v)| v)
                .unwrap_or(""),
        };
        // hyprpaper uses "wallpaper = DP-1,/path" — take the last token.
        let token = value
            .rsplit(',')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"')
            .trim();
        if token.is_empty() {
            continue;
        }
        if !(token.contains('/') || has_image_ext(token)) {
            continue;
        }
        let p = PathBuf::from(token);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn has_image_ext(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".webp", ".bmp", ".gif", ".tiff", ".avif", ".qoi",
    ]
    .iter()
    .any(|e| lower.ends_with(e))
}
