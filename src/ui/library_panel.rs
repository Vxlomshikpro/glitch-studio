//! Левая панель — браузер медиатеки.
//!
//! Показывает папки, поиск/фильтр/сортировку и виртуализированную сетку
//! миниатюр. Тяжёлые операции (скан, генерация миниатюр) уходят в фоновые
//! потоки через `ThumbnailCache`; здесь только чтение готового и запрос нового.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use super::{Action, AppState};
use crate::core::frame::Frame;
use crate::library::{Filter, MediaItem, SortKey, ThumbnailCache};
use crate::theme::Palette;
use crate::video::MediaKind;

/// Сколько миниатюр максимум запрашиваем за один кадр.
const MAX_REQUESTS_PER_FRAME: usize = 24;
/// Сколько текстур держим в кэше egui-памяти (LRU по порядку вставки).
const MAX_THUMB_TEXTURES: usize = 192;
/// Минимальная ширина ячейки сетки.
const MIN_CELL_W: f32 = 118.0;
/// Высота подписи под миниатюрой.
const NAME_H: f32 = 16.0;

/// Кэш текстур миниатюр, живущий в памяти egui-контекста.
#[derive(Clone, Default)]
struct ThumbTextures {
    map: HashMap<PathBuf, egui::TextureHandle>,
    order: VecDeque<PathBuf>,
}

fn thumb_tex_id() -> egui::Id {
    egui::Id::new("glitch_studio::library::thumb_tex")
}

fn selected_id() -> egui::Id {
    egui::Id::new("glitch_studio::library::selected")
}

/// Отрисовка панели.
pub fn show(ui: &mut egui::Ui, st: &mut AppState) {
    let p = st.palette.clone();
    header(ui, st, &p);
    ui.add_space(6.0);
    controls(ui, st, &p);
    ui.add_space(6.0);
    folders(ui, st, &p);
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);
    grid(ui, st, &p);
}

fn header(ui: &mut egui::Ui, st: &mut AppState, p: &Palette) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Библиотека")
                .strong()
                .size(15.0)
                .color(p.on_surface),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Обновить").clicked() {
                st.rescan();
            }
        });
    });
    let total = st.library.items.len();
    let shown = st.library.visible().len();
    ui.label(
        egui::RichText::new(format!("{shown} из {total}"))
            .small()
            .color(p.on_surface_variant),
    );
}

fn controls(ui: &mut egui::Ui, st: &mut AppState, p: &Palette) {
    let w = ui.available_width();
    ui.add(
        egui::TextEdit::singleline(&mut st.library.query)
            .hint_text("Поиск")
            .desired_width(w),
    );
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        for (f, label) in [
            (Filter::All, "Все"),
            (Filter::Images, "Фото"),
            (Filter::Videos, "Видео"),
        ] {
            if ui.selectable_label(st.library.filter == f, label).clicked() {
                st.library.filter = f;
            }
        }
    });

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Сортировка")
                .small()
                .color(p.on_surface_variant),
        );
        let cur = match st.library.sort {
            SortKey::Name => "Имя",
            SortKey::Date => "Дата",
            SortKey::Size => "Размер",
        };
        egui::ComboBox::from_id_salt("lib_sort")
            .selected_text(cur)
            .show_ui(ui, |ui| {
                for (k, label) in [
                    (SortKey::Name, "Имя"),
                    (SortKey::Date, "Дата"),
                    (SortKey::Size, "Размер"),
                ] {
                    if ui.selectable_label(st.library.sort == k, label).clicked() {
                        st.library.sort = k;
                    }
                }
            });
        let arrow = if st.library.ascending { "↑" } else { "↓" };
        if ui.button(arrow).clicked() {
            st.library.ascending = !st.library.ascending;
        }
    });
}

fn folders(ui: &mut egui::Ui, st: &mut AppState, p: &Palette) {
    let n = st.library.folders.len();
    egui::CollapsingHeader::new(format!("Папки ({n})"))
        .default_open(false)
        .show(ui, |ui| {
            if n == 0 {
                ui.label(
                    egui::RichText::new("Папок нет")
                        .small()
                        .color(p.on_surface_variant),
                );
            }
            let mut remove: Option<PathBuf> = None;
            for folder in &st.library.folders {
                ui.horizontal(|ui| {
                    let name = folder
                        .file_name()
                        .and_then(|s| s.to_str())
                        .or_else(|| folder.to_str())
                        .unwrap_or("?");
                    ui.label(egui::RichText::new(name).small());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").clicked() {
                            remove = Some(folder.clone());
                        }
                    });
                });
            }
            if let Some(path) = remove {
                st.library.remove_folder(&path);
                st.settings.library_folders = st.library.folders.clone();
                let _ = st.settings.save();
            }
            if ui.button("Добавить папку").clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    st.pending.push(Action::AddFolder(dir));
                }
            }
        });
}

fn grid(ui: &mut egui::Ui, st: &mut AppState, p: &Palette) {
    let items = st.library.visible();
    if items.is_empty() {
        let msg = if st.library.items.is_empty() {
            "Библиотека пуста — добавь папку"
        } else {
            "Ничего не найдено"
        };
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(msg).color(p.on_surface_variant));
        });
        return;
    }

    let ctx = ui.ctx().clone();
    let thumbs = &mut st.thumbs;
    let pending = &mut st.pending;
    let thumb_size = st.thumb_size;
    let selected = ctx
        .data(|d| d.get_temp::<Option<PathBuf>>(selected_id()))
        .flatten();

    let avail = ui.available_width();
    let cols = ((avail / MIN_CELL_W).floor() as usize).clamp(1, 3);
    let spacing = ui.spacing().item_spacing.x;
    let cell_w = ((avail - spacing * (cols as f32 - 1.0)) / cols as f32).max(48.0);
    let row_h = cell_w + NAME_H;
    let total_rows = items.len().div_ceil(cols);

    let mut missing: Vec<PathBuf> = Vec::new();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_h, total_rows, |ui, range| {
            for row in range {
                ui.horizontal(|ui| {
                    for c in 0..cols {
                        let idx = row * cols + c;
                        let Some(&item) = items.get(idx) else {
                            break;
                        };
                        cell(
                            ui,
                            &ctx,
                            thumbs,
                            pending,
                            selected.as_deref(),
                            item,
                            cell_w,
                            p,
                            &mut missing,
                        );
                    }
                });
            }
        });

    if !missing.is_empty() {
        thumbs.request_async(missing, thumb_size);
    }
}

#[allow(clippy::too_many_arguments)]
fn cell(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    thumbs: &mut ThumbnailCache,
    pending: &mut Vec<Action>,
    selected: Option<&Path>,
    item: &MediaItem,
    cell_w: f32,
    p: &Palette,
    missing: &mut Vec<PathBuf>,
) {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(cell_w, cell_w + NAME_H), egui::Sense::click());
    let img_rect = egui::Rect::from_min_size(rect.min, egui::vec2(cell_w, cell_w));
    let name_rect = egui::Rect::from_min_max(egui::pos2(rect.min.x, img_rect.max.y), rect.max);

    let is_sel = selected == Some(item.path.as_path());

    // 1) кэш текстур в памяти egui
    let cached = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<ThumbTextures>(thumb_tex_id())
            .map
            .get(&item.path)
            .cloned()
    });

    let tex = match cached {
        Some(t) => Some(t),
        None => match thumbs.get(&item.path) {
            Some(frame) => {
                let t = make_texture(ctx, &item.path, &frame);
                if let Some(t) = t.as_ref() {
                    ctx.data_mut(|d| {
                        let cache = d.get_temp_mut_or_default::<ThumbTextures>(thumb_tex_id());
                        if cache.map.len() >= MAX_THUMB_TEXTURES {
                            if let Some(old) = cache.order.pop_front() {
                                cache.map.remove(&old);
                            }
                        }
                        cache.map.insert(item.path.clone(), t.clone());
                        cache.order.push_back(item.path.clone());
                    });
                }
                t
            }
            None => {
                if missing.len() < MAX_REQUESTS_PER_FRAME {
                    missing.push(item.path.clone());
                }
                None
            }
        },
    };

    let painter = ui.painter();
    if let Some(t) = tex.as_ref() {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(t.id(), img_rect, uv, egui::Color32::WHITE);
    } else {
        painter.rect_filled(
            img_rect,
            egui::CornerRadius::same(4),
            p.surface_container_high,
        );
        painter.text(
            img_rect.center(),
            egui::Align2::CENTER_CENTER,
            "…",
            egui::FontId::proportional(18.0),
            p.on_surface_variant,
        );
    }

    if item.kind == MediaKind::Video {
        let badge = egui::Rect::from_min_size(
            egui::pos2(img_rect.left() + 4.0, img_rect.top() + 4.0),
            egui::vec2(16.0, 16.0),
        );
        painter.rect_filled(
            badge,
            egui::CornerRadius::same(3),
            p.surface_container_low.gamma_multiply(0.85),
        );
        painter.text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            "▶",
            egui::FontId::proportional(10.0),
            p.on_surface,
        );
    }

    let stroke = if is_sel {
        egui::Stroke::new(2.0, p.primary)
    } else {
        egui::Stroke::new(1.0, p.outline)
    };
    painter.rect_stroke(
        img_rect,
        egui::CornerRadius::same(4),
        stroke,
        egui::StrokeKind::Inside,
    );

    let name = truncate_name(&item.name, 16);
    let name_color = if is_sel {
        p.primary
    } else {
        p.on_surface_variant
    };
    painter.text(
        name_rect.center(),
        egui::Align2::CENTER_CENTER,
        name,
        egui::FontId::proportional(11.0),
        name_color,
    );

    if resp.clicked() {
        ctx.data_mut(|d| d.insert_temp(selected_id(), Some(item.path.clone())));
    }
    if resp.double_clicked() {
        pending.push(Action::OpenFile(item.path.clone()));
    }

    resp.on_hover_ui(|ui| {
        ui.label(egui::RichText::new(&item.name).strong());
        ui.label(
            egui::RichText::new(format!(
                "{} · {}",
                human_size(item.size_bytes),
                human_date(item.modified_secs)
            ))
            .small()
            .color(p.on_surface_variant),
        );
        if item.kind == MediaKind::Video {
            match item.info.as_ref() {
                Some(info) => {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}×{} · {}",
                            info.width,
                            info.height,
                            fmt_dur(info.duration_secs)
                        ))
                        .small(),
                    );
                }
                None => {
                    ui.label(
                        egui::RichText::new("инфо недоступно")
                            .small()
                            .color(p.on_surface_variant),
                    );
                }
            }
        }
    });
}

fn make_texture(ctx: &egui::Context, path: &Path, frame: &Frame) -> Option<egui::TextureHandle> {
    let w = frame.width as usize;
    let h = frame.height as usize;
    if w == 0 || h == 0 {
        return None;
    }
    let expected = w.checked_mul(h).and_then(|n| n.checked_mul(4));
    if expected != Some(frame.data.len()) {
        return None;
    }
    let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &frame.data);
    Some(ctx.load_texture(
        format!("thumb:{}", path.display()),
        img,
        egui::TextureOptions::LINEAR,
    ))
}

fn truncate_name(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} ГБ", b / GB)
    } else if b >= MB {
        format!("{:.1} МБ", b / MB)
    } else if b >= KB {
        format!("{:.0} КБ", b / KB)
    } else {
        format!("{bytes} Б")
    }
}

fn human_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Дни от эпохи -> (год, месяц, день). Алгоритм Говарда Хиннанта.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn fmt_dur(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "0:00".to_string();
    }
    let total = secs.round() as i64;
    format!("{}:{:02}", total / 60, total % 60)
}
