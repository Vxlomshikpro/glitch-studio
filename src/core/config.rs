//! Пути приложения и постоянные настройки.

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

fn dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "GlitchStudio")
}

pub fn config_dir() -> PathBuf {
    dirs()
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join(".glitch-studio"))
}

pub fn cache_dir() -> PathBuf {
    dirs()
        .map(|d| d.cache_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join(".glitch-studio-cache"))
}

pub fn data_dir() -> PathBuf {
    dirs()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join(".glitch-studio-data"))
}

pub fn config_file() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn presets_file() -> PathBuf {
    config_dir().join("presets.json")
}

pub fn thumbs_dir() -> PathBuf {
    cache_dir().join("thumbs")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub library_folders: Vec<PathBuf>,
    pub recent_files: Vec<PathBuf>,
    pub export_dir: Option<PathBuf>,
    pub theme: String,
    pub theme_light: bool,
    pub palette_source: PaletteSource,
    pub custom_seed_color: String,
    pub animations_on: bool,
    pub scanline_overlay: bool,
    pub preview_max_width: u32,
    pub jpeg_quality: u8,
    pub default_fps: u32,
    pub video_quality: u8,
    pub window_w: f32,
    pub window_h: f32,
}

impl Default for Settings {
    fn default() -> Self {
        let export_dir = data_dir().join("exports");
        Self {
            library_folders: Vec::new(),
            recent_files: Vec::new(),
            export_dir: Some(export_dir),
            theme: "Violet Dark".to_string(),
            theme_light: false,
            palette_source: PaletteSource::Builtin,
            custom_seed_color: "#b39dff".to_string(),
            animations_on: true,
            scanline_overlay: true,
            preview_max_width: 1280,
            jpeg_quality: 92,
            default_fps: 30,
            video_quality: 85,
            window_w: 1400.0,
            window_h: 880.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaletteSource {
    Builtin,
    FromImage,
    FromWallpaper,
    CustomColor,
}

impl Settings {
    pub fn load() -> Self {
        let path = config_file();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = config_file();
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(
            &path,
            serde_json::to_string_pretty(self).unwrap_or_default(),
        )
    }

    /// Папка библиотеки, куда попадают пресеты и настройки, — создаётся на лету.
    pub fn ensure_dirs(&self) {
        let _ = std::fs::create_dir_all(config_dir());
        let _ = std::fs::create_dir_all(thumbs_dir());
        if let Some(d) = &self.export_dir {
            let _ = std::fs::create_dir_all(d);
        }
    }

    pub fn push_recent(&mut self, path: PathBuf) {
        self.recent_files.retain(|p| *p != path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(12);
    }
}
