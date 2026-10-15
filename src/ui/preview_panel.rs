//! Центральная область: превью, разделитель «до/после», таймлайн видео.

use super::AppState;

pub fn show(ui: &mut egui::Ui, st: &mut AppState) {
    let ctx = ui.ctx().clone();
    let p = st.p().clone();

    if !st.has_media() {
        show_placeholder(ui, st);
        return;
    }

    let available = ui.available_size();
    let show_timeline = st.player.is_open();

    let preview_height = if show_timeline {
        (available.y - 92.0).max(120.0)
    } else {
        available.y
    };

    let (response, painter) = ui.allocate_painter(
        egui::vec2(available.x, preview_height),
        egui::Sense::hover(),
    );
    let rect = response.rect;

    // Фон-подложка: тёмный, в тон темы.
    painter.rect_filled(rect, egui::CornerRadius::same(8), p.surface_container_low);

    // Drag & drop файла прямо в окно.
    let mut dropped: Vec<std::path::PathBuf> = Vec::new();
    for f in ctx.input(|i| i.raw.dropped_files.clone()) {
        dropped.push(f.path().to_path_buf());
    }
    if let Some(path) = dropped
        .into_iter()
        .find(|p| super::super::core::media::is_media(p))
    {
        st.pending.push(super::Action::OpenFile(path));
    }

    let show_original = st.show_original;
    let original_tex = if show_original {
        st.original_texture_of(&ctx)
    } else {
        None
    };
    let tex = st.texture.clone();

    let split = st.split_pos.unwrap_or(1.0).clamp(0.0, 1.0);
    let split_px = rect.width() * split;

    // Раскладка внутри: слева «после», справа «до», если разделитель не у края.
    let mut after_rect = rect;
    let mut before_rect = egui::Rect::NOTHING;
    let mut split_rect = egui::Rect::NOTHING;
    if split > 0.01 && split < 0.99 {
        before_rect =
            egui::Rect::from_min_max(egui::pos2(rect.max.x - split_px, rect.min.y), rect.max);
        after_rect =
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x - split_px, rect.max.y));
        split_rect = egui::Rect::from_min_max(
            egui::pos2(before_rect.min.x - 1.0, rect.min.y),
            egui::pos2(before_rect.min.x + 1.0, rect.max.y),
        );
    }

    if let Some(t) = tex.as_ref() {
        fit_image(&painter, after_rect, t, &p);
    } else {
        draw_placeholder_text(&painter, after_rect, "считаю…", &p);
    }

    if let Some(t) = original_tex.as_ref() {
        fit_image(&painter, before_rect, t, &p);
        // Перечёркиваем «до» кислотной линией, чтобы не путать с результатом.
        let c = if st.settings.theme_light {
            p.error
        } else {
            p.tertiary
        };
        painter.line_segment(
            [before_rect.left_top(), before_rect.right_bottom()],
            egui::Stroke::new(1.5, c),
        );
    }

    if split_rect.width() > 0.0 {
        painter.rect_filled(
            split_rect.expand(2.0),
            egui::CornerRadius::same(2),
            p.primary.gamma_multiply(0.25),
        );
        painter.rect_filled(split_rect, egui::CornerRadius::ZERO, p.primary);
    }

    if st.settings.scanline_overlay {
        let phase = st.phase as f32;
        crate::theme::scanline_overlay(&painter, rect, 0.035, phase);
    }

    // Перетаскивание разделителя.
    if let Some(pos) = response.interact_pointer_pos() {
        if pos.x > rect.min.x + 4.0 && pos.x < rect.max.x - 4.0 {
            st.split_pos = Some(((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0));
        }
    }

    // Заголовок с именем файла и индикатором расчёта.
    let name = st
        .current
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("—");
    painter.text(
        egui::pos2(rect.left() + 8.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::monospace(12.0),
        p.on_surface_variant,
    );

    if st.engine.busy {
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.min.x, rect.max.y - 2.0),
            egui::vec2(rect.width(), 2.0),
        );
        painter.rect_filled(bar, egui::CornerRadius::ZERO, p.primary.gamma_multiply(0.4));
    }

    if show_timeline {
        show_timeline_panel(ui, st, &p);
    }
}

fn show_placeholder(ui: &mut egui::Ui, st: &mut AppState) {
    let p = st.p().clone();
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);

    painter.rect_filled(rect, egui::CornerRadius::same(8), p.surface_container_low);

    let center = rect.center();
    let hints: Vec<(&str, egui::Color32)> = vec![
        ("Перетащи файл сюда", p.on_surface),
        ("или Ctrl+O — открыть", p.on_surface_variant),
        (
            "Ctrl+Shift+O — добавить папку в библиотеку",
            p.on_surface_variant,
        ),
    ];

    for (i, (text, color)) in hints.iter().enumerate() {
        let y = center.y - 30.0 + i as f32 * 22.0;
        let galley =
            ui.painter()
                .layout_no_wrap(text.to_string(), egui::FontId::proportional(15.0), *color);
        ui.painter().galley(
            egui::pos2(center.x - galley.size().x / 2.0, y - galley.size().y / 2.0),
            galley,
            *color,
        );
    }

    // Кнопки быстрого старта.
    let btn_y = center.y + 34.0;
    // egui 0.36: allocate_rect только резервирует прямоугольник, для вложенного
    // Ui нужен scope_builder + new_child.
    let btn_rect =
        egui::Rect::from_min_size(egui::pos2(center.x - 90.0, btn_y), egui::vec2(180.0, 28.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(btn_rect), |ui: &mut egui::Ui| {
            ui.horizontal(|ui: &mut egui::Ui| {
                if ui.button("Открыть файл").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter(
                            "Медиа",
                            &[
                                "png", "jpg", "jpeg", "webp", "bmp", "gif", "mp4", "mkv",
                                "webm", "mov",
                            ],
                        )
                        .pick_file()
                    {
                        st.pending.push(super::Action::OpenFile(path));
                    }
                }
                if ui.button("Папка").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        st.pending.push(super::Action::AddFolder(dir));
                    }
                }
            });
        });
}

/// Вписывает текстуру в прямоугольник с сохранением пропорций.
fn fit_image(
    painter: &egui::Painter,
    rect: egui::Rect,
    tex: &egui::TextureHandle,
    p: &crate::theme::Palette,
) {
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let size = tex.size_vec2();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let w = size.x * scale;
    let h = size.y * scale;
    let target = egui::Rect::from_center_size(rect.center(), egui::vec2(w, h));
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    // Тон должен быть нейтральным: цвет темы здесь гасил бы яркость картинки.
    painter.image(tex.id(), target, uv, egui::Color32::WHITE);
    let _ = p;
}

fn draw_placeholder_text(
    painter: &egui::Painter,
    rect: egui::Rect,
    text: &str,
    p: &crate::theme::Palette,
) {
    let galley = painter.layout_no_wrap(
        text.to_string(),
        egui::FontId::monospace(14.0),
        p.on_surface_variant,
    );
    painter.galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        p.on_surface_variant,
    );
}

fn show_timeline_panel(ui: &mut egui::Ui, st: &mut AppState, p: &crate::theme::Palette) {
    ui.add_space(6.0);
    let total = st.player.total_frames();
    let current = st.player.frame_no.min(total.max(1));

    ui.horizontal(|ui| {
        let play_label = if st.player.playing { "⏸" } else { "▶" };
        if ui
            .add_sized([34.0, 26.0], egui::Button::new(play_label))
            .clicked()
        {
            st.player.playing = !st.player.playing;
        }
        if ui.button("⏮").clicked() {
            st.player.step(-10);
            st.request_preview(true);
        }
        if ui.button("⏭").clicked() {
            st.player.step(10);
            st.request_preview(true);
        }
        ui.label(format!(
            "{} / {}",
            fmt_time(st.player.current_seconds()),
            fmt_time(st.player.duration())
        ));
    });

    let mut slider_pos = current as f32;
    let resp =
        ui.add(egui::Slider::new(&mut slider_pos, 0.0..=total.max(1) as f32).show_value(false));
    if resp.changed() {
        st.player.seek_to(slider_pos as u64);
        st.request_preview(true);
    }

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("кадр {}", current))
                .monospace()
                .color(p.on_surface_variant),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{:.0} fps", st.player.fps()))
                    .monospace()
                    .color(p.on_surface_variant),
            );
        });
    });
}

fn fmt_time(sec: f64) -> String {
    if !sec.is_finite() || sec < 0.0 {
        return "0:00".to_string();
    }
    let total = sec.round() as i64;
    format!("{}:{:02}", total / 60, total % 60)
}
