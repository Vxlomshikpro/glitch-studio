//! Окна: настройки, экспорт видео, сохранение пресета.

use super::AppState;
use crate::core::config::PaletteSource;
use crate::theme;
use std::path::PathBuf;

pub fn open_save_as(st: &mut AppState) {
    let start = st
        .current
        .as_ref()
        .and_then(|p| p.parent().map(|x| x.to_path_buf()))
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
        .unwrap_or_default();
    if let Some(p) = rfd::FileDialog::new()
        .add_filter("Изображение", &["png", "jpg", "jpeg", "webp"])
        .add_filter("Видео", &["mp4", "webm", "gif"])
        .set_directory(start)
        .set_file_name("glitch.png")
        .save_file()
    {
        st.save_as(p);
    }
}

pub fn prompt_preset_name(st: &mut AppState) {
    st.preset_dialog = true;
    st.preset_name.clear();
}

pub fn show(ctx: &egui::Context, st: &mut AppState) {
    if st.preset_dialog {
        let mut open = true;
        egui::Window::new("Имя пресета")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Как назвать пресет?");
                ui.add(
                    egui::TextEdit::singleline(&mut st.preset_name)
                        .hint_text("например, Мой VHS")
                        .desired_width(240.0),
                );
                ui.add_space(6.0);
                let name = st.preset_name.trim().to_string();
                ui.horizontal(|ui| {
                    if ui.button("Сохранить").clicked() && !name.is_empty() {
                        st.save_current_preset(&name);
                        st.preset_dialog = false;
                    }
                    if ui.button("Отмена").clicked() {
                        st.preset_dialog = false;
                    }
                });
            });
        if !open {
            st.preset_dialog = false;
        }
    }

    if st.settings_open {
        settings_window(ctx, st);
    }
    if st.export_open {
        export_window(ctx, st);
    }
}

fn settings_window(ctx: &egui::Context, st: &mut AppState) {
    let mut open = true;
    let mut changed_palette = false;
    let mut changed_preview = false;

    egui::Window::new("Настройки")
        .open(&mut open)
        .resizable(true)
        .default_width(380.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let p = st.p().clone();

            ui.label(
                egui::RichText::new("ТЕМА")
                    .strong()
                    .color(p.primary)
                    .size(11.0),
            );
            let mut theme_pick = st.settings.theme.clone();
            egui::ComboBox::from_id_salt("settings-theme")
                .selected_text(egui::RichText::new(theme_label(&theme_pick)).monospace())
                .show_ui(ui, |ui| {
                    for (id, label) in theme::builtin_themes() {
                        if ui.selectable_label(id == theme_pick, label).clicked() {
                            theme_pick = id;
                        }
                    }
                });
            if theme_pick != st.settings.theme {
                st.set_theme(&theme_pick);
                changed_palette = true;
            }

            ui.horizontal(|ui| {
                if ui
                    .add_sized(
                        [70.0, 24.0],
                        egui::Button::new(if st.settings.theme_light {
                            "☾ тёмная"
                        } else {
                            "☀ светлая"
                        }),
                    )
                    .clicked()
                {
                    st.settings.theme_light = !st.settings.theme_light;
                    changed_palette = true;
                }
            });

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("ПАЛИТРА")
                    .strong()
                    .color(p.primary)
                    .size(11.0),
            );
            egui::ComboBox::from_id_salt("palette-source")
                .selected_text(palette_label(st.settings.palette_source))
                .show_ui(ui, |ui| {
                    for (src, label) in [
                        (PaletteSource::Builtin, "Встроенная"),
                        (PaletteSource::FromImage, "Из текущего изображения"),
                        (PaletteSource::FromWallpaper, "Из обоев рабочего стола"),
                        (PaletteSource::CustomColor, "Свой цвет"),
                    ] {
                        if ui
                            .selectable_label(st.settings.palette_source == src, label)
                            .clicked()
                        {
                            st.settings.palette_source = src;
                        }
                    }
                });
            if st.settings.palette_source == PaletteSource::CustomColor {
                ui.horizontal(|ui| {
                    ui.label("hex");
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut st.settings.custom_seed_color)
                                .desired_width(100.0),
                        )
                        .changed()
                    {
                        changed_palette = true;
                    }
                });
            }
            if st.settings.palette_source == PaletteSource::FromImage && !st.has_media() {
                ui.label(
                    egui::RichText::new("Открой фото — палитра возьмётся из него")
                        .size(11.0)
                        .color(p.error),
                );
            }
            if changed_palette {
                st.rebuild_palette();
            }

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("ОФОРМЛЕНИЕ")
                    .strong()
                    .color(p.primary)
                    .size(11.0),
            );
            let mut anim = st.settings.animations_on;
            if ui.checkbox(&mut anim, "Анимации глитча").changed() {
                st.settings.animations_on = anim;
            }
            let mut scan = st.settings.scanline_overlay;
            if ui.checkbox(&mut scan, "Оверлей скан-линий").changed() {
                st.settings.scanline_overlay = scan;
            }

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("КАЧЕСТВО")
                    .strong()
                    .color(p.primary)
                    .size(11.0),
            );
            let mut jq = st.settings.jpeg_quality as f32;
            if ui
                .add(
                    egui::Slider::new(&mut jq, 1.0..=100.0)
                        .suffix("%")
                        .text("JPEG"),
                )
                .changed()
            {
                st.settings.jpeg_quality = jq.round().clamp(1.0, 100.0) as u8;
            }
            let mut vq = st.settings.video_quality as f32;
            if ui
                .add(
                    egui::Slider::new(&mut vq, 1.0..=100.0)
                        .suffix("%")
                        .text("Видео"),
                )
                .changed()
            {
                st.settings.video_quality = vq.round().clamp(1.0, 100.0) as u8;
            }
            let mut fps = st.settings.default_fps as f32;
            if ui
                .add(egui::Slider::new(&mut fps, 1.0..=60.0).text("FPS"))
                .changed()
            {
                st.settings.default_fps = fps.round().clamp(1.0, 60.0) as u32;
            }
            let mut pw = st.settings.preview_max_width as f32;
            if ui
                .add(
                    egui::Slider::new(&mut pw, 320.0..=2560.0)
                        .step_by(80.0)
                        .suffix("px")
                        .text("Ширина превью"),
                )
                .changed()
            {
                st.settings.preview_max_width = pw.round() as u32;
                changed_preview = true;
            }

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("ПАПКА ЭКСПОРТА")
                    .strong()
                    .color(p.primary)
                    .size(11.0),
            );
            let dir_text = st
                .settings
                .export_dir
                .as_ref()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|| "не выбрана".to_string());
            ui.label(egui::RichText::new(dir_text).monospace().size(10.5));
            if ui.button("Выбрать папку…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    let _ = std::fs::create_dir_all(&d);
                    st.settings.export_dir = Some(d);
                }
            }

            if changed_preview {
                st.invalidate_preview();
                st.request_preview(true);
            }
        });

    if !open {
        st.settings_open = false;
    }
}

fn export_window(ctx: &egui::Context, st: &mut AppState) {
    let mut open = true;
    egui::Window::new("Экспорт видео")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(360.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let p = st.p().clone();

            if !st.has_media() {
                ui.label("Сначала открой видео.");
                if ui.button("Закрыть").clicked() {
                    st.export_open = false;
                }
                return;
            }
            if !st.player.is_open() {
                ui.label("Экспорт видео работает только для видеофайлов.");
                if ui.button("Закрыть").clicked() {
                    st.export_open = false;
                }
                return;
            }
            if st.export_state.is_some() {
                ui.label("Экспорт уже идёт.");
                if ui.button("Отменить").clicked() {
                    st.cancel_export();
                    st.export_open = false;
                }
                return;
            }

            let mut format = st
                .export_to
                .as_ref()
                .and_then(|p| crate::core::media::ext_of(p))
                .unwrap_or_else(|| "mp4".to_string());
            egui::ComboBox::from_id_salt("export-format")
                .selected_text(&format)
                .show_ui(ui, |ui| {
                    for f in ["mp4", "webm", "gif"] {
                        if ui.selectable_label(format == f, f).clicked() {
                            format = f.to_string();
                        }
                    }
                });

            let mut fps = st.settings.default_fps as f32;
            ui.add(egui::Slider::new(&mut fps, 1.0..=60.0).text("FPS"));
            let fps = fps.round().clamp(1.0, 60.0) as u32;
            let mut quality = st.settings.video_quality as f32;
            ui.add(
                egui::Slider::new(&mut quality, 1.0..=100.0)
                    .suffix("%")
                    .text("Качество"),
            );
            let quality = quality.round().clamp(1.0, 100.0) as u8;

            let src_w = st
                .player
                .full_frame()
                .map(|f| f.width)
                .or_else(|| st.player.info.as_ref().map(|i| i.width))
                .unwrap_or(1920);
            let src_h = st
                .player
                .full_frame()
                .map(|f| f.height)
                .or_else(|| st.player.info.as_ref().map(|i| i.height))
                .unwrap_or(1080);

            let mut size_idx = 0usize;
            let labels = [
                format!("оригинал {src_w}×{src_h}"),
                "1280×720".to_string(),
                "1920×1080".to_string(),
                "512×512".to_string(),
            ];
            egui::ComboBox::from_id_salt("export-size")
                .selected_text(&labels[size_idx])
                .show_ui(ui, |ui| {
                    for (i, l) in labels.iter().enumerate() {
                        if ui.selectable_label(size_idx == i, l).clicked() {
                            size_idx = i;
                        }
                    }
                });

            ui.add_space(6.0);
            let suggested = st
                .export_to
                .clone()
                .unwrap_or_else(|| default_export_path(st, &format));
            ui.label(
                egui::RichText::new(suggested.display().to_string())
                    .monospace()
                    .size(10.0)
                    .color(p.on_surface_variant),
            );

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Начать экспорт").clicked() {
                    st.settings.default_fps = fps;
                    st.settings.video_quality = quality;
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Видео", &["mp4", "webm", "gif"])
                        .set_directory(
                            st.settings
                                .export_dir
                                .clone()
                                .unwrap_or_else(|| PathBuf::from(".")),
                        )
                        .set_file_name(
                            default_export_path(st, &format)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("out.mp4")
                                .to_string(),
                        )
                        .save_file()
                    {
                        st.start_export(path);
                        st.export_open = false;
                    }
                }
                if ui.button("Отмена").clicked() {
                    st.export_open = false;
                }
            });

        });

    if !open {
        st.export_open = false;
    }
}

fn default_export_path(st: &AppState, ext: &str) -> PathBuf {
    let dir = st
        .settings
        .export_dir
        .clone()
        .unwrap_or_else(|| crate::core::config::data_dir().join("exports"));
    let stem = st
        .current
        .as_ref()
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or("video")
        .to_string();
    crate::core::media::next_export_path(&dir, &stem, ext)
}

fn palette_label(src: PaletteSource) -> &'static str {
    match src {
        PaletteSource::Builtin => "Встроенная",
        PaletteSource::FromImage => "Из текущего изображения",
        PaletteSource::FromWallpaper => "Из обоев рабочего стола",
        PaletteSource::CustomColor => "Свой цвет",
    }
}

fn theme_label(id: &str) -> String {
    theme::builtin_themes()
        .into_iter()
        .find(|(k, _)| k == id)
        .map(|(_, v)| v)
        .unwrap_or_else(|| id.to_string())
}
