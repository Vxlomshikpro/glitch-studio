//! Рекурсивный обход папок: собирает медиафайлы, не падая на ошибках доступа.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::core::media;
use crate::video::MediaKind;

use super::store::MediaItem;

/// Глубина рекурсии. Защита от патологических деревьев и симлинк-петель.
const MAX_DEPTH: usize = 16;

/// Обходит все папки рекурсивно и возвращает найденные медиафайлы.
/// Скрытые каталоги, симлинк-петли и слишком глубокие уровни пропускаются;
/// ошибки доступа и исчезнувшие файлы молча игнорируются.
pub fn scan(folders: &[PathBuf]) -> Vec<MediaItem> {
    let mut out = Vec::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    for folder in folders {
        walk(folder, 0, &mut out, &mut visited);
    }
    out
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<MediaItem>, visited: &mut HashSet<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let canon = match std::fs::canonicalize(dir) {
        Ok(c) => c,
        Err(_) => return,
    };
    if !visited.insert(canon) {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_dir() {
            if !is_hidden(&entry) {
                walk(&path, depth + 1, out, visited);
            }
        } else if ft.is_symlink() {
            match std::fs::metadata(&path) {
                Ok(m) if m.is_dir() => {
                    if !is_hidden(&entry) {
                        walk(&path, depth + 1, out, visited);
                    }
                }
                Ok(m) if m.is_file() => {
                    if let Some(item) = make_item(&path, &m) {
                        out.push(item);
                    }
                }
                _ => {}
            }
        } else if ft.is_file() {
            if let Ok(m) = entry.metadata() {
                if let Some(item) = make_item(&path, &m) {
                    out.push(item);
                }
            }
        }
    }
}

fn is_hidden(entry: &std::fs::DirEntry) -> bool {
    entry.file_name().to_string_lossy().starts_with('.')
}

fn make_item(path: &Path, meta: &std::fs::Metadata) -> Option<MediaItem> {
    if !meta.is_file() {
        return None;
    }
    let size_bytes = meta.len();
    if size_bytes == 0 {
        return None;
    }
    if !media::is_media(path) {
        return None;
    }
    let ext = media::ext_of(path)?;
    let kind = if media::is_video_ext(&ext) {
        MediaKind::Video
    } else {
        MediaKind::Image
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let modified_secs = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some(MediaItem {
        path: path.to_path_buf(),
        name,
        size_bytes,
        modified_secs,
        kind,
        info: None,
    })
}
