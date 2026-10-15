//! Интерфейс Glitch Studio.
//!
//! Раскладка: слева медиа-библиотека, в центре превью с таймлайном,
//! справа ползунки выбранного эффекта, сверху панель действий.
//! Стек эффектов — отдельное окно (см. `effect_stack`).

pub mod dialogs;
pub mod effect_stack;
pub mod engine;
pub mod hotkeys;
pub mod inspector;
pub mod library_panel;
pub mod player;
pub mod preview_panel;
pub mod topbar;
pub mod widgets;

use std::path::PathBuf;
use std::sync::Arc;

use crate::core::config::{PaletteSource, Settings};
use crate::core::frame::Frame;
use crate::core::media;
use crate::core::pipeline::Pipeline;
use crate::core::preset::{Preset, PresetLibrary};
use crate::core::registry::Registry;
use crate::core::seed::Rng;
use crate::library::{Library, MediaItem, ThumbnailCache};
use crate::theme::{self, Palette};

/// Действия, которые панели возвращают в главный цикл.
#[derive(Debug, Clone)]
pub enum Action {
    OpenFile(PathBuf),
    ShowFolder(PathBuf),
    AddFolder(PathBuf),
    Save,
    SaveAs,
    Randomize,
    NewSeed,
    NextTheme,
    ToggleLeftPanel,
    ToggleRightPanel,
    LoadPreset(usize),
    OpenSettings,
    OpenExport,
    SavePreset,
    DeletePreset,
    ResetStack,
    ExportPreset,
    ImportPreset,
    OpenHotkeys,
}

#[derive(PartialEq, Clone, Copy)]
pub enum ToastKind {
    Info,
    Ok,
    Error,
}

impl ToastKind {
    pub fn color(self, p: &Palette) -> egui::Color32 {
        match self {
            Self::Info => p.on_surface_variant,
            Self::Ok => p.tertiary,
            Self::Error => p.error,
        }
    }
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub born: f64,
    pub action: Option<(&'static str, Action)>,
}

pub struct ExportState {
    pub progress: Arc<std::sync::atomic::AtomicU32>,
    pub total: u64,
    pub cancel: crate::video::Cancel,
    pub stage: &'static str,
    pub done: bool,
    pub failed: bool,
}

pub struct AppState {
    pub settings: Settings,
    pub registry: Arc<Registry>,
    pub pipeline: Pipeline,
    pub presets: PresetLibrary,
    pub user_presets: Vec<Preset>,
    pub selected_preset: usize,

    pub library: Library,
    pub thumbs: ThumbnailCache,
    pub scan_handle: Option<std::thread::JoinHandle<Vec<MediaItem>>>,
    pub scanning: bool,
    pub scanned_once: bool,
    pub thumb_size: u32,

    pub current: Option<PathBuf>,
    pub source: Option<Frame>,
    pub preview_source: Option<Arc<Frame>>,
    pub preview_result: Option<Frame>,
    pub texture: Option<egui::TextureHandle>,
    pub original_texture: Option<egui::TextureHandle>,

    pub player: player::Player,
    pub engine: engine::PreviewEngine,

    pub selected: Option<usize>,
    pub show_original: bool,
    pub left_panel: bool,
    pub right_panel: bool,
    pub split_pos: Option<f32>,

    pub theme_name: String,
    pub palette: Palette,
    pub phase: f64,
    pub now: f64,

    pub toast: Option<Toast>,
    pub settings_open: bool,
    pub export_open: bool,
    pub hotkeys_open: bool,
    pub stack_open: bool,

    pub export_state: Option<ExportState>,
    pub export_to: Option<PathBuf>,
    pub preset_dialog: bool,
    pub preset_name: String,
    pub pending: Vec<Action>,

    undo: Vec<Pipeline>,
    redo: Vec<Pipeline>,
    last_hash: Option<u64>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let settings = Settings::load();
        settings.ensure_dirs();
        let registry = Arc::new(Registry::new());
        let user_presets = crate::cli::load_user_presets();
        let presets = PresetLibrary::load(&user_presets);

        // Стек по умолчанию: почти всё выключено. Иначе превью считало бы все
        // 30 эффектов на каждом кадре и подвисало. Пользователь включает нужные
        // сам, пресеты включают свой набор.
        let mut pipeline = Pipeline::with_defaults(42, &registry);
        pipeline.reconcile(&registry);
        for entry in pipeline.stack.iter_mut() {
            entry.enabled = matches!(entry.id.as_str(), "rgb_split" | "scanlines");
            entry.mix = if entry.enabled { 0.6 } else { 1.0 };
        }

        let theme_name = settings.theme.clone();
        let palette = Palette::builtin(&theme_name, settings.theme_light);
        let library = Library::new(settings.library_folders.clone());
        let engine = engine::PreviewEngine::new(Arc::clone(&registry), player::HISTORY);

        Self {
            settings,
            registry,
            pipeline,
            presets,
            user_presets,
            selected_preset: 0,
            library,
            thumbs: ThumbnailCache::new(),
            scan_handle: None,
            scanning: false,
            scanned_once: false,
            thumb_size: 128,
            current: None,
            source: None,
            preview_source: None,
            preview_result: None,
            texture: None,
            original_texture: None,
            player: player::Player::default(),
            engine,
            selected: None,
            show_original: false,
            left_panel: true,
            right_panel: true,
            split_pos: None,
            theme_name,
            palette,
            phase: 0.0,
            now: 0.0,
            toast: None,
            settings_open: false,
            export_open: false,
            hotkeys_open: false,
            stack_open: false,
            export_state: None,
            export_to: None,
            preset_dialog: false,
            preset_name: String::new(),
            pending: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_hash: None,
        }
    }

    pub fn p(&self) -> &Palette {
        &self.palette
    }

    pub fn has_media(&self) -> bool {
        self.current.is_some()
    }

    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            born: self.now,
            action: None,
        });
    }

    pub fn toast_with(
        &mut self,
        text: impl Into<String>,
        kind: ToastKind,
        action: (&'static str, Action),
    ) {
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            born: self.now,
            action: Some(action),
        });
    }

    // ---- тема ---------------------------------------------------------------

    pub fn set_theme(&mut self, name: &str) {
        self.theme_name = name.to_string();
        self.settings.theme = name.to_string();
        self.rebuild_palette();
        let _ = self.settings.save();
    }

    pub fn next_theme(&mut self) {
        let names = theme::theme_names();
        if names.is_empty() {
            return;
        }
        let cur = names.iter().position(|n| *n == self.theme_name).unwrap_or(0);
        self.set_theme(&names[(cur + 1) % names.len()]);
    }

    pub fn rebuild_palette(&mut self) {
        let light = self.settings.theme_light;
        let next = match self.settings.palette_source {
            PaletteSource::CustomColor => {
                let (r, g, b) = parse_hex(&self.settings.custom_seed_color);
                Some(Palette::from_seed_rgb(r, g, b, light))
            }
            PaletteSource::FromImage => self
                .preview_source
                .as_ref()
                .and_then(|f| theme::extract_from_image(f, light)),
            PaletteSource::FromWallpaper => theme::extract_from_wallpaper(light),
            PaletteSource::Builtin => None,
        };
        self.palette = next
            .unwrap_or_else(|| Palette::builtin(&self.theme_name, light));
    }

    // ---- медиа --------------------------------------------------------------

    pub fn open(&mut self, path: PathBuf) {
        let ext = media::ext_of(&path).unwrap_or_default();
        if !media::is_media(&path) {
            self.toast(format!("не поддерживается: {ext}"), ToastKind::Error);
            return;
        }

        if media::is_video_ext(&ext) {
            let info = crate::video::probe(&path).ok();
            self.player.preview_width = self.settings.preview_max_width.clamp(320, 1280);
            match self.player.open(path.clone(), info) {
                Ok(()) => {
                    if let Some(f) = self.player.frame() {
                        self.preview_source = Some(Arc::new(engine::preview_source(
                            f,
                            self.settings.preview_max_width,
                        )));
                    }
                }
                Err(e) => {
                    self.toast(format!("не открылось: {e}"), ToastKind::Error);
                    return;
                }
            }
        } else {
            match media::load(&path) {
                Ok(f) => {
                    self.source = Some(f);
                    self.preview_source = Some(Arc::new(engine::preview_source(
                        self.source.as_ref().unwrap_or(&Frame::new(1, 1)),
                        self.settings.preview_max_width,
                    )));
                }
                Err(e) => {
                    self.toast(format!("не открылось: {e}"), ToastKind::Error);
                    return;
                }
            }
        }

        self.current = Some(path.clone());
        self.settings.push_recent(path);
        self.engine.invalidate();
        self.texture = None;
        self.original_texture = None;
        if self.settings.palette_source == PaletteSource::FromImage {
            self.rebuild_palette();
        }
        self.request_preview(true);
        let _ = self.settings.save();
    }

    pub fn rescan(&mut self) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        let folders = self.library.folders.clone();
        self.scan_handle = std::thread::Builder::new()
            .name("gs-scan".to_string())
            .spawn(move || crate::library::scan(&folders))
            .ok();
    }

    pub fn poll_scan(&mut self) {
        if !self.scanning {
            return;
        }
        let done = self
            .scan_handle
            .as_ref()
            .map(|h| h.is_finished())
            .unwrap_or(true);
        if !done {
            return;
        }
        if let Some(h) = self.scan_handle.take() {
            if let Ok(items) = h.join() {
                self.library.items = items;
                self.scanning = false;
            } else {
                self.scanning = false;
            }
        }
    }

    // ---- превью -------------------------------------------------------------

    pub fn request_preview(&mut self, force: bool) {
        let Some(src) = self.preview_source.clone() else {
            self.preview_result = None;
            self.texture = None;
            return;
        };
        let hash = self.pipeline.content_hash();
        if !force && self.last_hash == Some(hash) {
            return;
        }
        self.last_hash = Some(hash);
        let history = self.player.history();
        self.engine.request(src, &self.pipeline, history);
    }

    pub fn poll_preview(&mut self, ctx: &egui::Context) {
        if self.engine.poll() {
            self.preview_result = self.engine.current.clone();
            self.texture = None;
        }
        if self.texture.is_none() {
            if let Some(f) = self.preview_result.as_ref() {
                let img = egui::ColorImage::from_rgba_unmultiplied(
                    [f.width as usize, f.height as usize],
                    &f.data,
                );
                self.texture =
                    Some(ctx.load_texture("preview", img, egui::TextureOptions::LINEAR));
            }
        }
    }

    pub fn original_texture_of(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        let frame = self.source.as_ref().or_else(|| self.player.frame())?;
        if self.original_texture.is_some() {
            return self.original_texture.clone();
        }
        let img = egui::ColorImage::from_rgba_unmultiplied(
            [frame.width as usize, frame.height as usize],
            &frame.data,
        );
        let tex = ctx.load_texture("original", img, egui::TextureOptions::LINEAR);
        self.original_texture = Some(tex.clone());
        Some(tex)
    }

    pub fn invalidate_preview(&mut self) {
        self.last_hash = None;
        self.texture = None;
    }

    // ---- правка -------------------------------------------------------------

    pub fn push_undo(&mut self) {
        self.undo.push(self.pipeline.clone());
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some(p) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.pipeline, p));
            self.after_change();
        }
    }

    pub fn redo(&mut self) {
        if let Some(p) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.pipeline, p));
            self.after_change();
        }
    }

    pub fn after_change(&mut self) {
        self.pipeline.reconcile(&self.registry);
        self.request_preview(true);
    }

    pub fn randomize(&mut self) {
        self.push_undo();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        self.pipeline.seed = Rng::new(nanos).below(u32::MAX) as u64;
        self.after_change();
    }

    pub fn new_seed(&mut self) {
        self.push_undo();
        self.pipeline.seed = self.pipeline.seed.wrapping_add(1);
        self.after_change();
    }

    pub fn apply_preset(&mut self, index: usize) {
        let Some(preset) = self.presets.presets.get(index).cloned() else {
            return;
        };
        self.push_undo();
        self.pipeline = preset.into_pipeline(&self.registry);
        self.selected_preset = index;
        self.after_change();
    }

    pub fn save_current_preset(&mut self, name: &str) {
        self.user_presets.retain(|p| p.name != name);
        let preset = Preset::new(name, self.pipeline.clone());
        self.user_presets.push(preset.clone());
        self.presets.save(preset);
        let lib = PresetLibrary {
            presets: self.user_presets.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&lib) {
            let path = crate::core::config::presets_file();
            if let Some(p) = path.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::write(path, json);
        }
        self.toast(format!("пресет «{name}» сохранён"), ToastKind::Ok);
    }

    // ---- сохранение ---------------------------------------------------------

    pub fn quick_save(&mut self) {
        let Some(current) = self.current.clone() else {
            self.toast("нечего сохранять — открой файл", ToastKind::Error);
            return;
        };
        let ext_src = media::ext_of(&current).unwrap_or_default();
        if media::is_video_ext(&ext_src) {
            self.export_open = true;
            return;
        }
        let Some(frame) = self.render_full() else {
            self.toast("нет кадра для сохранения", ToastKind::Error);
            return;
        };
        let dir = self
            .settings
            .export_dir
            .clone()
            .unwrap_or_else(|| crate::core::config::data_dir().join("exports"));
        let stem = current
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("glitch")
            .to_string();
        let path = media::next_export_path(&dir, &stem, "png");

        match media::save(&frame, &path, self.settings.jpeg_quality) {
            Ok(()) => {
                let folder = path.parent().map(|p| p.to_path_buf());
                let text = format!("сохранено: {}", path.display());
                match folder {
                    Some(f) => {
                        self.toast_with(text, ToastKind::Ok, ("Показать в папке", Action::ShowFolder(f)))
                    }
                    None => self.toast(text, ToastKind::Ok),
                }
            }
            Err(e) => self.toast(format!("не сохранилось: {e}"), ToastKind::Error),
        }
    }

    pub fn save_as(&mut self, path: PathBuf) {
        let Some(frame) = self.render_full() else {
            self.toast("нет кадра для сохранения", ToastKind::Error);
            return;
        };
        let ext = media::ext_of(&path).unwrap_or_else(|| "png".to_string());
        if media::is_video_ext(&ext) {
            self.export_to = Some(path);
            self.export_open = true;
            return;
        }
        match media::save(&frame, &path, self.settings.jpeg_quality) {
            Ok(()) => {
                let folder = path.parent().map(|p| p.to_path_buf());
                let text = format!("сохранено: {}", path.display());
                match folder {
                    Some(f) => {
                        self.toast_with(text, ToastKind::Ok, ("Показать в папке", Action::ShowFolder(f)))
                    }
                    None => self.toast(text, ToastKind::Ok),
                }
            }
            Err(e) => self.toast(format!("не сохранилось: {e}"), ToastKind::Error),
        }
    }

    /// Кадр в полном разрешении с применённым пайплайном.
    pub fn render_full(&self) -> Option<Frame> {
        if let Some(src) = self.source.as_ref() {
            return self.pipeline.apply(src, &self.registry, &[]).ok();
        }
        if let Some(f) = self.player.full_frame() {
            let history = self.player.history();
            return self.pipeline.apply(f, &self.registry, &history).ok();
        }
        None
    }

    // ---- экспорт видео ------------------------------------------------------

    pub fn start_export(&mut self, output: PathBuf) {
        if self.export_state.is_some() {
            self.toast("экспорт уже идёт", ToastKind::Info);
            return;
        }
        let Some(input) = self.current.clone() else {
            self.toast("видео не открыто", ToastKind::Error);
            return;
        };
        if !crate::video::ffmpeg_available() {
            self.toast("ffmpeg не найден — поставь через apt", ToastKind::Error);
            return;
        }

        let pipeline = self.pipeline.clone();
        let registry = Arc::clone(&self.registry);
        let opts = crate::video::ExportOptions {
            fps: self.settings.default_fps,
            quality: self.settings.video_quality,
            width: 0,
            height: 0,
            keep_audio: true,
            gif: media::ext_of(&output).as_deref() == Some("gif"),
            datamosh: false,
            history_frames: player::HISTORY,
        };
        let cancel = crate::video::Cancel::new();
        let cancel2 = cancel.clone();
        let progress = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let progress2 = Arc::clone(&progress);
        let total = self.player.total_frames();

        std::thread::Builder::new()
            .name("gs-export".to_string())
            .spawn(move || {
                let _ = crate::video::export_video(
                    &input,
                    &output,
                    &pipeline,
                    &registry,
                    &opts,
                    &cancel2,
                    &move |p| {
                        progress2.store(p.frame as u32, std::sync::atomic::Ordering::Relaxed);
                    },
                );
            })
            .ok();

        self.export_state = Some(ExportState {
            progress,
            total,
            cancel,
            stage: "Экспорт",
            done: false,
            failed: false,
        });
    }

    pub fn cancel_export(&mut self) {
        if let Some(s) = &self.export_state {
            s.cancel.cancel();
        }
    }

    pub fn poll_export(&mut self) {
        let Some(s) = &self.export_state else { return };
        let v = s.progress.load(std::sync::atomic::Ordering::Relaxed);
        if v == u32::MAX {
            self.export_state = None;
            self.toast("экспорт закончен", ToastKind::Ok);
            return;
        }
        if s.cancel.is_cancelled() {
            self.export_state = None;
            self.toast("экспорт отменён", ToastKind::Info);
        }
    }
}

pub fn parse_hex(s: &str) -> (u8, u8, u8) {
    let hex = s.trim().trim_start_matches('#');
    let get = |i: usize| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    if hex.len() >= 6 {
        (get(0), get(2), get(4))
    } else {
        (0, 0, 0)
    }
}

// ---- запуск ----------------------------------------------------------------

pub fn run_gui(cli: crate::cli::Cli) -> anyhow::Result<()> {
    let settings = Settings::load();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Glitch Studio")
        .with_inner_size([settings.window_w, settings.window_h])
        .with_min_inner_size([1100.0, 700.0])
        .with_resizable(true);

    // Иконка встроена в бинарь: иначе на Windows exe запущенный из чужой
    // директории не найдёт assets/ по относительному пути.
    const ICON_128: &[u8] = include_bytes!("../../assets/icons/icon-128.rgba");
    viewport = viewport.with_icon(egui::IconData {
        rgba: ICON_128.to_vec(),
        width: 128,
        height: 128,
    });

    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        multisampling: 0,
        ..Default::default()
    };

    // Файл из командной строки открываем сразу после создания окна.
    let start_with = cli.input.clone();

    match eframe::run_native(
        "Glitch Studio",
        options,
        Box::new(move |_cc| {
            let mut app = GlitchApp::new();
            if let Some(path) = start_with.clone() {
                app.state.open(path);
            }
            Ok(Box::new(app) as Box<dyn eframe::App>)
        }),
    ) {
        Ok(()) => Ok(()),
        Err(e) => {
            // «egl/не найден OpenGL» в виртуалке или на старой карте — самое
            // частое. Молчаливый выход здесь недопустим: объясняем, что делать.
            report_startup_failure(&e.to_string());
            Err(anyhow::anyhow!("{e}"))
        }
    }
}

/// Показывает понятное окно с причиной, если окно не удалось открыть.
fn report_startup_failure(msg: &str) {
    let lowered = msg.to_ascii_lowercase();
    let hint = if lowered.contains("opengl") {
        "Нужен OpenGL 2.0 или новее.\n\nЧаще всего это значит, что программа запущена в виртуалке \
         без 3D-ускорения, либо на машине с очень старой видеокартой.\n\nЧто делать:\n\
         • В виртуалке включи 3D-ускорение (VirtualBox: Настройки → Видео).\n\
         • Поставь свежий драйвер видеокарты.\n\
         • Как обходной путь: используй версию для командной строки."
    } else if lowered.contains("libfuse") {
        "Не хватает FUSE для запуска AppImage."
    } else {
        "Смотри сообщение ниже."
    };

    let title = "Glitch Studio не смог запуститься";
    let body = format!("{hint}\n\nТехнические детали:\n{msg}");

    eprintln!("{title}\n{body}");

    #[cfg(target_os = "windows")]
    {
        let _ = rfd::MessageDialog::new()
            .set_title(title)
            .set_description(&body)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }

    #[cfg(not(target_os = "windows"))]
    {
        // На других системах окно покажет терминал, но и без GUI-диалога
        // текст ошибки полезнее, чем ничего.
        let _ = (title, body);
    }
}

struct GlitchApp {
    state: AppState,
    last_save: f64,
}

impl GlitchApp {
    fn new() -> Self {
        Self {
            state: AppState::new(),
            last_save: 0.0,
        }
    }
}

impl eframe::App for GlitchApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let dt = ui.input(|i| i.stable_dt);
        let st = &mut self.state;
        st.now += dt as f64;
        st.phase = if st.settings.animations_on { st.now } else { 0.0 };

        if !st.scanned_once {
            st.scanned_once = true;
            if !st.library.folders.is_empty() {
                st.rescan();
            }
        }
        st.poll_scan();
        st.poll_export();
        st.thumbs.drain_ready();
        if st.player.tick() {
            // Движок сам перезапишет слот, если уже считает: очередь кадров
            // не растёт, а на экране всегда свежайший доступный кадр.
            st.request_preview(true);
        }
        st.poll_preview(ui.ctx());

        // egui перерисовывается только по событиям. Пока движок считает кадр,
        // без request_repaint результат может «зависнуть» на экране навсегда.
        if st.engine.busy {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
        }

        let mut all = hotkeys::collect(ui.ctx(), st);
        all.append(&mut st.pending);
        st.pending.clear();
        for a in all {
            apply_action(st, a);
        }

        theme::apply(ui.ctx(), &st.palette);

        let h = st.pipeline.content_hash();
        if st.last_hash != Some(h) {
            st.request_preview(true);
        }

        egui::Panel::top("topbar")
            .resizable(false)
            .exact_size(44.0)
            .show(ui, |ui| topbar::show(ui, st));

        if st.left_panel {
            // max_size обязателен: сетка миниатюр без него распирает панель
            // на всю ширину окна.
            egui::Panel::left("library")
                .resizable(true)
                .default_size(250.0)
                .min_size(180.0)
                .max_size(420.0)
                .show(ui, |ui| library_panel::show(ui, st));
        }

        if st.right_panel {
            egui::Panel::right("inspector")
                .resizable(true)
                .default_size(310.0)
                .min_size(240.0)
                .max_size(460.0)
                .show(ui, |ui| inspector::show(ui, st));
        }

        egui::CentralPanel::default_margins().show(ui, |ui| preview_panel::show(ui, st));

        effect_stack::show_window(ui.ctx(), st);
        dialogs::show(ui.ctx(), st);
        hotkeys::show_window(ui.ctx(), st);
        widgets::show_toast(ui.ctx(), st);

        let _ = frame;
        if st.now - self.last_save > 5.0 {
            self.last_save = st.now;
            let _ = st.settings.save();
        }
    }
}

fn apply_action(st: &mut AppState, a: Action) {
    match a {
        Action::OpenFile(p) => st.open(p),
        Action::ShowFolder(p) => {
            let _ = std::process::Command::new("xdg-open").arg(p).spawn();
        }
        Action::AddFolder(p) => {
            st.library.add_folder(p.clone());
            st.settings.library_folders = st.library.folders.clone();
            let _ = st.settings.save();
            st.rescan();
        }
        Action::Save => st.quick_save(),
        Action::SaveAs => dialogs::open_save_as(st),
        Action::Randomize => st.randomize(),
        Action::NewSeed => st.new_seed(),
        Action::NextTheme => st.next_theme(),
        Action::ToggleLeftPanel => st.left_panel = !st.left_panel,
        Action::ToggleRightPanel => st.right_panel = !st.right_panel,
        Action::LoadPreset(i) => st.apply_preset(i),
        Action::OpenSettings => st.settings_open = true,
        Action::OpenExport => st.export_open = true,
        Action::SavePreset => dialogs::prompt_preset_name(st),
        Action::DeletePreset => {
            let idx = st.selected_preset;
            if let Some(name) = st.presets.presets.get(idx).map(|p| p.name.clone()) {
                st.presets.delete(&name);
                st.user_presets.retain(|p| p.name != name);
                st.toast(format!("пресет «{name}» удалён"), ToastKind::Info);
            }
        }
        Action::ResetStack => {
            st.push_undo();
            st.pipeline = Pipeline::with_defaults(st.pipeline.seed, &st.registry);
            st.after_change();
        }
        Action::ExportPreset => {
            if let Some(p) = st.presets.presets.get(st.selected_preset).cloned() {
                if let Ok(json) = p.to_json() {
                    if let Some(dir) = rfd::FileDialog::new()
                        .add_filter("Пресет", &["json"])
                        .set_file_name("preset.json")
                        .save_file()
                    {
                        match std::fs::write(&dir, json) {
                            Ok(()) => st.toast("пресет выгружен", ToastKind::Ok),
                            Err(e) => st.toast(format!("{e}"), ToastKind::Error),
                        }
                    }
                }
            }
        }
        Action::ImportPreset => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Пресет", &["json"])
                .pick_file()
            {
                match std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|s| Preset::from_json(&s).map_err(|e| e.to_string()))
                {
                    Ok(p) => {
                        let name = p.name.clone();
                        st.save_current_preset(&name);
                    }
                    Err(e) => st.toast(format!("битый пресет: {e}"), ToastKind::Error),
                }
            }
        }
        Action::OpenHotkeys => st.hotkeys_open = true,
    }
}
