//! Состояние библиотеки: элементы, фильтр, поиск, сортировка, персист папок.

use std::path::{Path, PathBuf};

use crate::video::{MediaInfo, MediaKind};

use super::scanner;

/// Один медиафайл в библиотеке. Дёшев для клонирования: `info` заполняется лениво.
#[derive(Debug, Clone)]
pub struct MediaItem {
    pub path: PathBuf,
    pub name: String,
    pub size_bytes: u64,
    pub modified_secs: u64,
    pub kind: MediaKind,
    pub info: Option<MediaInfo>,
}

impl MediaItem {
    /// "vhs_tape.mp4" -> "vhs_tape".
    pub fn stem(&self) -> String {
        self.path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.name.clone())
    }
}

/// Ключ сортировки сетки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Date,
    Size,
}

/// Фильтр по типу медиа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Images,
    Videos,
}

impl Filter {
    fn accepts(self, kind: MediaKind) -> bool {
        match self {
            Filter::All => true,
            Filter::Images => kind == MediaKind::Image,
            Filter::Videos => kind == MediaKind::Video,
        }
    }
}

/// Индекс библиотеки: отслеживаемые папки + собранные элементы + вид сетки.
pub struct Library {
    pub folders: Vec<PathBuf>,
    pub items: Vec<MediaItem>,
    pub filter: Filter,
    pub sort: SortKey,
    pub ascending: bool,
    pub query: String,
}

impl Library {
    pub fn new(folders: Vec<PathBuf>) -> Self {
        Self {
            folders,
            items: Vec::new(),
            filter: Filter::All,
            sort: SortKey::Name,
            ascending: true,
            query: String::new(),
        }
    }

    /// Полный пересбор индекса по всем папкам. Блокирующая — звать с рабочего потока.
    pub fn rescan(&mut self) {
        self.items = scanner::scan(&self.folders);
    }

    /// Элементы после фильтра, поиска и сортировки. Вызывается каждый кадр,
    /// поэтому аллокаций минимум: один Vec ссылок (+ ключи только для сортировки по имени).
    pub fn visible(&self) -> Vec<&MediaItem> {
        let q = self.query.trim().to_lowercase();
        let mut out: Vec<&MediaItem> = self
            .items
            .iter()
            .filter(|it| self.filter.accepts(it.kind))
            .filter(|it| q.is_empty() || it.name.to_lowercase().contains(&q))
            .collect();

        match self.sort {
            SortKey::Name => {
                let mut keyed: Vec<(String, &MediaItem)> = out
                    .into_iter()
                    .map(|it| (it.name.to_lowercase(), it))
                    .collect();
                keyed.sort_by(|a, b| a.0.cmp(&b.0));
                if !self.ascending {
                    keyed.reverse();
                }
                out = keyed.into_iter().map(|(_, it)| it).collect();
            }
            SortKey::Date => {
                out.sort_by_key(|a| a.modified_secs);
                if !self.ascending {
                    out.reverse();
                }
            }
            SortKey::Size => {
                out.sort_by_key(|a| a.size_bytes);
                if !self.ascending {
                    out.reverse();
                }
            }
        }
        out
    }

    /// Добавляет папку в список. Сканирование не запускает — UI сам решает, когда звать `rescan`.
    pub fn add_folder(&mut self, path: PathBuf) {
        if !self.folders.iter().any(|f| f == &path) {
            self.folders.push(path);
        }
    }

    /// Убирает папку и все её элементы из индекса.
    pub fn remove_folder(&mut self, path: &Path) {
        self.folders.retain(|f| f.as_path() != path);
        self.items.retain(|it| !it.path.starts_with(path));
    }

    pub fn find(&self, path: &Path) -> Option<&MediaItem> {
        self.items.iter().find(|it| it.path.as_path() == path)
    }

    /// Сериализует список папок в JSON-массив строк.
    pub fn folders_json(&self) -> String {
        serde_json::to_string(&self.folders).unwrap_or_else(|_| "[]".to_string())
    }

    /// Разбирает JSON-массив путей; мусор игнорируется.
    pub fn load_folders_from(s: &str) -> Vec<PathBuf> {
        serde_json::from_str(s).unwrap_or_default()
    }

    /// Ленивый probe для тултипа: считает один раз и кэширует в элементе.
    pub fn info_for(&mut self, path: &Path) -> Option<&MediaInfo> {
        let idx = self.items.iter().position(|it| it.path.as_path() == path)?;
        let need = self
            .items
            .get(idx)
            .map(|it| it.info.is_none())
            .unwrap_or(false);
        if need {
            let info = crate::video::probe(path).ok();
            if let Some(item) = self.items.get_mut(idx) {
                item.info = info;
            }
        }
        self.items.get(idx).and_then(|it| it.info.as_ref())
    }
}
