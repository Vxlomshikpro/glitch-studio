//! Кастомные виджеты: карточки, слайдеры с двойным кликом, уведомления.

use super::AppState;
use crate::core::params::ParamSpec;

const TOAST_TTL: f64 = 4.5;

/// Карточка с заголовком в стиле Material. Возвращает внутренний Ui.
pub fn section(ui: &mut egui::Ui, title: &str, st: &AppState) -> egui::InnerResponse<()> {
    let p = st.p();
    egui::Frame::new()
        .fill(p.surface_container)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title.to_uppercase())
                    .size(10.5)
                    .strong()
                    .color(p.primary),
            );
            ui.add_space(4.0);
        })
}

/// Значение в моноширинном шрифте.
pub fn mono(ui: &mut egui::Ui, text: impl Into<String>, color: egui::Color32) {
    ui.colored_label(color, egui::RichText::new(text.into()).monospace());
}

/// Слайдер с подписью слева и значением справа. Двойной клик по подписи или по
/// самому слайдеру сбрасывает значение к дефолту. Возвращает true при изменении.
pub fn param_slider(ui: &mut egui::Ui, spec: &ParamSpec, value: &mut f32) -> bool {
    let before = *value;
    let mut v = *value;

    let id = egui::Id::new(("gs-param", spec.key)).with(ui.id());
    ui.horizontal(|ui| {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.interact(rect, id, egui::Sense::click());
        ui.label(egui::RichText::new(spec.label).size(12.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(spec.format(*value))
                    .monospace()
                    .size(11.5),
            );
        });
        if resp.double_clicked() {
            v = spec.default;
        }
    });

    let response = ui.add(
        egui::Slider::new(&mut v, spec.min..=spec.max)
            .show_value(false)
            .step_by(f64::from(spec.step.max(0.0))),
    );
    if response.double_clicked() {
        v = spec.default;
    }

    if v != before {
        *value = v.clamp(spec.min, spec.max);
        return true;
    }
    false
}

/// Чип вкл/выкл для эффекта.
pub fn toggle_chip(ui: &mut egui::Ui, label: &str, on: bool) -> bool {
    let text = if on {
        format!("● {label}")
    } else {
        format!("○ {label}")
    };
    ui.add(egui::Button::new(egui::RichText::new(text).size(11.0)))
        .clicked()
}

/// Поле ввода числа. Мусор на входе игнорируется, а не считается ошибкой.
pub fn number(ui: &mut egui::Ui, value: &mut u64) -> bool {
    let mut text = value.to_string();
    let changed = ui
        .add(egui::TextEdit::singleline(&mut text).desired_width(90.0))
        .changed();
    if changed {
        if let Ok(v) = text.trim().parse::<u64>() {
            *value = v;
            return true;
        }
    }
    false
}

/// Уведомление в правом нижнем углу.
pub fn show_toast(ctx: &egui::Context, st: &mut AppState) {
    if st.toast.as_ref().is_none() {
        return;
    }
    if st.now - st.toast.as_ref().map(|t| t.born).unwrap_or(0.0) > TOAST_TTL {
        st.toast = None;
        return;
    }

    let p = st.p().clone();
    let (text, kind) = {
        let t = st.toast.as_ref().expect("проверено выше");
        (t.text.clone(), t.kind)
    };
    let color = kind.color(&p);
    let bg = p.surface_container_high;
    let action_label = st
        .toast
        .as_ref()
        .and_then(|t| t.action.as_ref().map(|(l, _)| l.to_string()));
    let action = st
        .toast
        .as_ref()
        .and_then(|t| t.action.as_ref().map(|(_, a)| a.clone()));

    egui::Area::new(egui::Id::new("gs-toast"))
        .anchor(egui::Align2::RIGHT_BOTTOM, [-18.0, -18.0])
        .order(egui::Order::Tooltip)
        .show(ctx, |ui| {
            let galley = ui.painter().layout_no_wrap(
                text.clone(),
                egui::FontId::proportional(13.0),
                p.on_surface,
            );
            let act_w = action_label
                .as_ref()
                .map(|l| l.chars().count() as f32 * 7.0 + 22.0)
                .unwrap_or(0.0);
            let w = galley.size().x + 30.0 + act_w;
            let (r, resp) =
                ui.allocate_exact_size(egui::vec2(w, galley.size().y + 18.0), egui::Sense::hover());
            let painter = ui.painter().with_clip_rect(r);
            painter.rect_filled(r, egui::CornerRadius::same(8), bg);
            painter.rect_filled(
                egui::Rect::from_min_size(r.min, egui::vec2(3.0, r.height())),
                egui::CornerRadius::same(2),
                color,
            );
            painter.galley(
                egui::pos2(r.left() + 14.0, r.center().y - galley.size().y / 2.0),
                galley,
                p.on_surface,
            );

            if let (Some(label), Some(act)) = (action_label, action) {
                let btn = egui::Rect::from_center_size(
                    egui::pos2(r.right() - act_w / 2.0, r.center().y),
                    egui::vec2(act_w - 12.0, 22.0),
                );
                let hover =
                    resp.hovered() && btn.contains(resp.hover_pos().unwrap_or(egui::Pos2::ZERO));
                let c = if hover { p.primary } else { p.tertiary };
                let tg =
                    ui.painter()
                        .layout_no_wrap(label.clone(), egui::FontId::proportional(12.0), c);
                painter.galley(
                    egui::pos2(
                        btn.center().x - tg.size().x / 2.0,
                        btn.center().y - tg.size().y / 2.0,
                    ),
                    tg,
                    c,
                );
                if hover && resp.clicked() {
                    st.pending.push(act);
                    st.toast = None;
                }
            }
        });
}

/// Лёгкий оверлей скан-линий поверх произвольного прямоугольника.
pub fn scanlines(painter: &egui::Painter, rect: egui::Rect, strength: f32, phase: f32) {
    crate::theme::scanline_overlay(painter, rect, strength, phase);
}
