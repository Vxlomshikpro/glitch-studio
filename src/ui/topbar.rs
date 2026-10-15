//! Верхняя панель: открыть, сохранить, пресеты, seed, chaos, тема.

use super::{Action, AppState};
use crate::core::config::PaletteSource;
use crate::theme;

pub fn show(ui: &mut egui::Ui, st: &mut AppState) {
    let p = st.p().clone();
    let width = ui.available_width();
    let compact = width < 1180.0;

    ui.horizontal_centered(|ui| {
        // Логотип с RGB-сдвигом. Сбой дискретный и редкий: непрерывный джиттер
        // на 60 к/с выглядел бы как дрожание панели, а не как глитч.
        // Под курсором сбой выключен — иначе панель «подпрыгивает» под мышью.
        let over_ui = ui.ctx().pointer_hover_pos().is_some_and(|p| ui.max_rect().contains(p));
        let amount = glitch_amount(st, over_ui);
        theme::glitch_title(ui, "GLITCH STUDIO", amount);

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        if ui
            .add_sized([62.0, 24.0], egui::Button::new("Открыть"))
            .clicked()
        {
            st.pending.push(Action::OpenFile(
                std::env::var("HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default(),
            ));
            st.pending.pop();
            st.hotkeys_open = false;
            open_dialog_now(st);
        }
        if ui
            .add_sized([62.0, 24.0], egui::Button::new("Папка"))
            .clicked()
        {
            add_folder_now(st);
        }
        if ui
            .add_sized([76.0, 24.0], egui::Button::new("Сохранить"))
            .clicked()
        {
            st.pending.push(Action::Save);
        }
        if ui
            .add_sized([68.0, 24.0], egui::Button::new("Save As"))
            .clicked()
        {
            st.pending.push(Action::SaveAs);
        }
        if ui
            .add_sized([68.0, 24.0], egui::Button::new("Экспорт"))
            .clicked()
        {
            st.pending.push(Action::OpenExport);
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);

        // Пресеты
        ui.menu_button("Пресеты", |ui| {
            let count = st.presets.presets.len();
            for (i, preset) in st.presets.presets.iter().enumerate().take(40) {
                let label = format!("{}{}", preset.name, if preset.builtin { "" } else { " *" });
                if ui
                    .selectable_label(st.selected_preset == i, label)
                    .clicked()
                {
                    st.pending.push(Action::LoadPreset(i));
                    ui.close();
                }
            }
            if count == 0 {
                ui.label("пусто");
            }
            ui.separator();
            if ui.button("Сохранить текущий как…").clicked() {
                st.pending.push(Action::SavePreset);
                ui.close();
            }
            if ui.button("Удалить выбранный").clicked() {
                st.pending.push(Action::DeletePreset);
                ui.close();
            }
            if ui.button("Экспорт в JSON…").clicked() {
                st.pending.push(Action::ExportPreset);
                ui.close();
            }
            if ui.button("Импорт из JSON…").clicked() {
                st.pending.push(Action::ImportPreset);
                ui.close();
            }
        });

        // Seed и chaos
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("seed")
                .size(11.0)
                .color(p.on_surface_variant),
        );
        let mut seed = st.pipeline.seed;
        if crate::ui::widgets::number(ui, &mut seed) {
            st.push_undo();
            st.pipeline.seed = seed;
            st.after_change();
        }
        if ui
            .add_sized([24.0, 22.0], egui::Button::new("🎲"))
            .clicked()
        {
            st.pending.push(Action::Randomize);
        }
        if ui
            .add_sized([28.0, 22.0], egui::Button::new("R+"))
            .clicked()
        {
            st.pending.push(Action::NewSeed);
        }

        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("chaos")
                .size(11.0)
                .color(p.on_surface_variant),
        );
        let mut chaos = st.pipeline.chaos;
        let resp = ui.add(
            egui::Slider::new(&mut chaos, 0.0..=100.0)
                .show_value(true)
                .suffix("%"),
        );
        if resp.changed() {
            st.pipeline.chaos = chaos.clamp(0.0, 100.0);
            st.after_change();
        }
        if resp.drag_started() {
            st.push_undo();
        }

        // Тема
        ui.add_space(6.0);
        let mut picked = st.theme_name.clone();
        egui::ComboBox::from_id_salt("theme")
            .selected_text(egui::RichText::new(picked.clone()).monospace().size(11.0))
            .width(if compact { 110.0 } else { 150.0 })
            .show_ui(ui, |ui| {
                for (id, label) in theme::builtin_themes() {
                    if ui.selectable_label(id == picked, label).clicked() {
                        picked = id;
                    }
                }
            });
        if picked != st.theme_name {
            st.set_theme(&picked);
        }
        if ui
            .add_sized(
                [26.0, 22.0],
                egui::Button::new(if st.settings.theme_light {
                    "☾"
                } else {
                    "☀"
                }),
            )
            .clicked()
        {
            st.settings.theme_light = !st.settings.theme_light;
            st.rebuild_palette();
        }

        // Источник палитры — компактно, иконкой.
        if ui
            .add_sized([26.0, 22.0], egui::Button::new(palette_icon(st)))
            .on_hover_text("Источник палитры")
            .clicked()
        {
            st.settings.palette_source = match st.settings.palette_source {
                PaletteSource::Builtin => PaletteSource::FromImage,
                PaletteSource::FromImage => PaletteSource::FromWallpaper,
                PaletteSource::FromWallpaper => PaletteSource::CustomColor,
                PaletteSource::CustomColor => PaletteSource::Builtin,
            };
            st.rebuild_palette();
        }

        // Панели и настройки
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_sized([24.0, 22.0], egui::Button::new("⚙")).clicked() {
                st.pending.push(Action::OpenSettings);
            }
            if ui.add_sized([24.0, 22.0], egui::Button::new("?")).clicked() {
                st.pending.push(Action::OpenHotkeys);
            }
            if ui
                .add_sized(
                    [24.0, 22.0],
                    egui::Button::new(if st.left_panel { "◧" } else { "▢" }),
                )
                .clicked()
            {
                st.pending.push(Action::ToggleLeftPanel);
            }
            if ui
                .add_sized(
                    [24.0, 22.0],
                    egui::Button::new(if st.right_panel { "◨" } else { "▢" }),
                )
                .clicked()
            {
                st.pending.push(Action::ToggleRightPanel);
            }
            if ui
                .add_sized([70.0, 22.0], egui::Button::new("Стек"))
                .clicked()
            {
                st.stack_open = !st.stack_open;
            }
        });

        // Прогресс экспорта — занимает всю свободную ширину.
        if let Some(export) = st.export_state.as_ref() {
            let v = export.progress.load(std::sync::atomic::Ordering::Relaxed);
            let done = v == u32::MAX;
            let total = export.total.max(1);
            let frac = if done { 1.0 } else { v as f32 / total as f32 };
            ui.add_space(10.0);
            ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).desired_width(120.0));
            if ui
                .add_sized([60.0, 22.0], egui::Button::new("Стоп"))
                .clicked()
            {
                st.cancel_export();
            }
        }
    });
}

/// Редкий дискретный «сбой» логотипа: примерно 1–2 импульса в секунду.
fn glitch_amount(st: &AppState, hovered: bool) -> f32 {
    if !st.settings.animations_on || hovered {
        return 0.0;
    }
    let tick = (st.phase / 0.45).floor() as u64;
    let h = tick.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (tick >> 3);
    if h % 5 == 0 {
        1.4
    } else if h % 7 == 0 {
        0.7
    } else {
        0.0
    }
}

fn palette_icon(st: &AppState) -> &'static str {
    match st.settings.palette_source {
        PaletteSource::Builtin => "◆",
        PaletteSource::FromImage => "▣",
        PaletteSource::FromWallpaper => "▤",
        PaletteSource::CustomColor => "✎",
    }
}

fn open_dialog_now(st: &mut AppState) {
    if let Some(p) = rfd::FileDialog::new()
        .add_filter(
            "Медиа",
            &[
                "png", "jpg", "jpeg", "webp", "bmp", "gif", "mp4", "mkv", "webm", "mov",
            ],
        )
        .set_directory(
            std::env::var("HOME")
                .ok()
                .map(std::path::PathBuf::from)
                .unwrap_or_default(),
        )
        .pick_file()
    {
        st.pending.push(Action::OpenFile(p));
    }
}

fn add_folder_now(st: &mut AppState) {
    if let Some(p) = rfd::FileDialog::new()
        .set_directory(
            std::env::var("HOME")
                .ok()
                .map(std::path::PathBuf::from)
                .unwrap_or_default(),
        )
        .pick_folder()
    {
        st.pending.push(Action::AddFolder(p));
    }
}
