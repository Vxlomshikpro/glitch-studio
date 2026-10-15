//! Фоновое создание миниатюр и дисковый кэш.
//!
//! Формат файла миниатюры (raw, без PNG):
//! `[4B magic "GTHM"][1B version=1][4B LE width][4B LE height][width*height*4 B RGBA8]`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use rayon::prelude::*;

use crate::core::config;
use crate::core::media;
use crate::core::Frame;

const MAGIC: &[u8; 4] = b"GTHM";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 13;
/// Потолок роста кэша: при превышении удаляются самые старые файлы по mtime.
const MAX_CACHE_BYTES: u64 = 512 * 1024 * 1024;
/// Мягкий предел in-memory карты, чтобы не съесть RAM на большой библиотеке.
const MAX_MEM_ENTRIES: usize = 256;

/// Дисковый кэш миниатюр + очередь готовых результатов от фоновых воркеров.
pub struct ThumbnailCache {
    dir: PathBuf,
    ready: Arc<Mutex<Vec<(PathBuf, Frame)>>>,
    pending: HashSet<PathBuf>,
    mem: HashMap<PathBuf, Frame>,
}

impl ThumbnailCache {
    /// Создаёт кэш в `config::thumbs_dir()`, каталог — по требованию.
    pub fn new() -> Self {
        let dir = config::thumbs_dir();
        let _ = std::fs::create_dir_all(&dir);
        enforce_cache_cap(&dir);
        Self {
            dir,
            ready: Arc::new(Mutex::new(Vec::new())),
            pending: HashSet::new(),
            mem: HashMap::new(),
        }
    }

    /// Готовая миниатюра из памяти или с диска, если она уже есть.
    pub fn get(&self, path: &Path) -> Option<Frame> {
        if let Some(f) = self.mem.get(path) {
            return Some(f.clone());
        }
        let meta = std::fs::metadata(path).ok()?;
        let file = self.dir.join(format!("{}.rgba", cache_key(path, &meta)));
        read_thumb(&file)
    }

    /// Генерирует одну миниатюру (картинка — decode+downscale, видео — кадр из
    /// середины) и кладёт её в кэш. Блокирующая.
    pub fn generate(&mut self, path: &Path, size: u32) -> Option<Frame> {
        let frame = generate_thumb(&self.dir, path, size)?;
        if self.mem.len() >= MAX_MEM_ENTRIES {
            self.mem.clear();
        }
        self.mem.insert(path.to_path_buf(), frame.clone());
        Some(frame)
    }

    /// Ставит пачку путей в очередь на rayon-пул. Возвращается сразу, не блокирует UI.
    pub fn request_async(&mut self, paths: Vec<PathBuf>, size: u32) {
        let mut to_do = Vec::new();
        for p in paths {
            if self.pending.insert(p.clone()) {
                to_do.push(p);
            }
        }
        if to_do.is_empty() {
            return;
        }
        let dir = self.dir.clone();
        let ready = Arc::clone(&self.ready);
        rayon::spawn(move || {
            to_do.into_par_iter().for_each(|p| {
                if let Some(frame) = generate_thumb(&dir, &p, size) {
                    if let Ok(mut v) = ready.lock() {
                        v.push((p, frame));
                    }
                }
            });
            enforce_cache_cap(&dir);
        });
    }

    /// Забирает готовые миниатюры, накопленные воркерами.
    pub fn drain_ready(&mut self) -> Vec<(PathBuf, Frame)> {
        let mut out = Vec::new();
        if let Ok(mut v) = self.ready.lock() {
            out.append(&mut v);
        }
        for (p, _) in &out {
            self.pending.remove(p);
        }
        out
    }
}

impl Default for ThumbnailCache {
    fn default() -> Self {
        Self::new()
    }
}

fn generate_thumb(dir: &Path, path: &Path, size: u32) -> Option<Frame> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let file = dir.join(format!("{}.rgba", cache_key(path, &meta)));
    if let Some(f) = read_thumb(&file) {
        return Some(f);
    }
    let frame = build_thumb(path, size)?;
    let _ = write_thumb(&file, &frame);
    Some(frame)
}

fn build_thumb(path: &Path, size: u32) -> Option<Frame> {
    let size = size.max(1);
    let ext = media::ext_of(path)?;
    let frame = if media::is_video_ext(&ext) {
        let info = crate::video::probe(path).ok()?;
        let mid = if info.duration_secs > 0.0 {
            info.duration_secs / 2.0
        } else {
            0.0
        };
        crate::video::grab_frame_at(path, mid).ok()??
    } else if media::is_image_ext(&ext) {
        media::load(path).ok()?
    } else {
        return None;
    };
    if frame.width == 0 || frame.height == 0 {
        return None;
    }
    Some(media::downscale(&frame, size, size))
}

fn read_thumb(file: &Path) -> Option<Frame> {
    let bytes = std::fs::read(file).ok()?;
    if bytes.len() < HEADER_LEN || bytes[0..4] != MAGIC[..] || bytes[4] != VERSION {
        return None;
    }
    let w = u32::from_le_bytes(bytes[5..9].try_into().ok()?);
    let h = u32::from_le_bytes(bytes[9..13].try_into().ok()?);
    Frame::from_rgba(w, h, bytes[HEADER_LEN..].to_vec())
}

fn write_thumb(file: &Path, frame: &Frame) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut buf = Vec::with_capacity(HEADER_LEN + frame.data.len());
    buf.extend_from_slice(MAGIC);
    buf.push(VERSION);
    buf.extend_from_slice(&frame.width.to_le_bytes());
    buf.extend_from_slice(&frame.height.to_le_bytes());
    buf.extend_from_slice(&frame.data);
    std::fs::write(file, buf)
}

/// Имя кэш-файла: FNV-1a от пути + mtime + размера. Смена файла меняет ключ.
fn cache_key(path: &Path, meta: &std::fs::Metadata) -> String {
    let size = meta.len().to_le_bytes();
    let (secs, nanos) = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| (d.as_secs(), d.subsec_nanos()))
        .unwrap_or((0, 0));
    let secs = secs.to_le_bytes();
    let nanos = nanos.to_le_bytes();
    let h = fnv1a_parts(&[
        path.as_os_str().as_encoded_bytes(),
        &size[..],
        &secs[..],
        &nanos[..],
    ]);
    format!("{h:016x}")
}

fn fnv1a_parts(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for &b in *part {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn enforce_cache_cap(dir: &Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut files: Vec<(u64, PathBuf, u64)> = Vec::new();
    let mut total: u64 = 0;
    for entry in entries.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();
        total = total.saturating_add(size);
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        files.push((mtime, entry.path(), size));
    }
    if total <= MAX_CACHE_BYTES {
        return;
    }
    files.sort_by_key(|(mtime, _, _)| *mtime);
    for (_, path, size) in files {
        if total <= MAX_CACHE_BYTES {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}
