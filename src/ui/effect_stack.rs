//! Плавающее окно стека эффектов.
//!
//! Показывает порядок применения эффектов, позволяет добавлять, удалять,
//! переставлять, включать/выключать и менять микс. Все изменения применяются
//! после обхода списка (собираем операции в `Vec<StackOp>`), чтобы не ловить
//! конфликты заимствований на `st.pipeline.stack`.

use std::sync::Arc;

use super::{Action, AppState};
use crate::core::effect::EffectGroup;
use crate::core::pipeline::StackEntry;
use crate::core::registry::Registry;
use crate::theme::Palette;

/// Отложенная операция над стеком.
enum StackOp {
    Move(usize, usize),
    Remove(usize),
    Select(usize),
    Toggle(usize, bool),
    SetMix(usize, f32),
}

/// Отрисовка окна. Ничего не делает, если окно закрыто.
pub fn show_window(ctx: &egui::Context, st: &mut AppState) {
    if !st.stack_open {
        return;
    }
    let p = st.palette.clone();
    let mut open = true;
    egui::Window::new("Стек эффектов")
        .open(&mut open)
        .default_size([440.0, 540.0])
        .min_width(380.0)
        .resizable(true)
        .show(ctx, |ui| {
            body(ui, st, &p);
        });
    if !open {
        st.stack_open = false;
    }
}

fn body(ui: &mut egui::Ui, st: &mut AppState, p: &Palette) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("seed {}", st.pipeline.seed))
                .monospace()
                .color(p.on_surface_variant),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Сброс").clicked() {
                st.pending.push(Action::ResetStack);
            }
            if ui.button("Случайно").clicked() {
                st.pending.push(Action::Randomize);
            }
            if ui.button("Новый seed").clicked() {
                st.pending.push(Action::NewSeed);
            }
        });
    });

    let mut chaos = st.pipeline.chaos;
    if ui
        .add(
            egui::Slider::new(&mut chaos, 0.0..=100.0)
                .text("Хаос")
                .suffix("%"),
        )
        .changed()
    {
        st.pipeline.chaos = chaos;
        st.after_change();
    }

    ui.separator();

    let registry = Arc::clone(&st.registry);
    if let Some(id) = add_row(ui, &registry, p) {
        if let Ok(eff) = registry.get(&id) {
            st.push_undo();
            st.pipeline.stack.push(StackEntry::new(&id, eff.defaults()));
            st.after_change();
        }
    }

    ui.separator();

    list(ui, st, &registry, p);
}

fn add_row(ui: &mut egui::Ui, registry: &Registry, p: &Palette) -> Option<String> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Добавить:").color(p.on_surface_variant));
        egui::ComboBox::from_id_salt("add_effect")
            .selected_text("выбрать эффект")
            .width(240.0)
            .show_ui(ui, |ui| {
                for group in EffectGroup::ALL {
                    ui.label(
                        egui::RichText::new(group.label())
                            .small()
                            .strong()
                            .color(p.on_surface_variant),
                    );
                    for eff in registry.all() {
                        if eff.group() != group {
                            continue;
                        }
                        if ui.selectable_label(false, eff.name()).clicked() {
                            chosen = Some(eff.id().to_string());
                        }
                    }
                    ui.separator();
                }
            });
    });
    chosen
}

fn list(ui: &mut egui::Ui, st: &mut AppState, registry: &Registry, p: &Palette) {
    let selected = st.selected;
    let mut ops: Vec<StackOp> = Vec::new();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let count = st.pipeline.stack.len();
            if count == 0 {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Стек пуст — добавь эффект сверху")
                        .color(p.on_surface_variant),
                );
                return;
            }
            for i in 0..count {
                let (id, enabled, mix) = {
                    let e = &st.pipeline.stack[i];
                    (e.id.clone(), e.enabled, e.mix)
                };
                let name = registry
                    .get(&id)
                    .map(|e| e.name())
                    .unwrap_or("неизвестный эффект");
                let is_sel = selected == Some(i);
                let fill = if is_sel {
                    p.primary_container
                } else {
                    p.surface_container_low
                };
                let stroke_c = if is_sel { p.primary } else { p.outline };

                egui::Frame::NONE
                    .fill(fill)
                    .stroke(egui::Stroke::new(1.0, stroke_c))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(i > 0, egui::Button::new("▲").small())
                                .clicked()
                            {
                                ops.push(StackOp::Move(i, i - 1));
                            }
                            if ui
                                .add_enabled(i + 1 < count, egui::Button::new("▼").small())
                                .clicked()
                            {
                                ops.push(StackOp::Move(i, i + 1));
                            }
                            let mut en = enabled;
                            if ui.checkbox(&mut en, "").changed() {
                                ops.push(StackOp::Toggle(i, en));
                            }
                            if ui
                                .selectable_label(is_sel, egui::RichText::new(name).strong())
                                .clicked()
                            {
                                ops.push(StackOp::Select(i));
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("✕").clicked() {
                                        ops.push(StackOp::Remove(i));
                                    }
                                },
                            );
                        });
                        let mut m = mix;
                        if ui
                            .add(egui::Slider::new(&mut m, 0.0..=1.0).text("Микс"))
                            .changed()
                        {
                            ops.push(StackOp::SetMix(i, m));
                        }
                    });
                ui.add_space(4.0);
            }
        });

    let mut structural = false;
    for op in ops {
        match op {
            StackOp::Select(i) => {
                st.selected = Some(i);
            }
            StackOp::Move(a, b) => {
                st.pipeline.move_entry(a, b);
                structural = true;
            }
            StackOp::Remove(i) => {
                if i < st.pipeline.stack.len() {
                    st.pipeline.stack.remove(i);
                    structural = true;
                }
                if st.selected == Some(i) {
                    st.selected = None;
                }
            }
            StackOp::Toggle(i, v) => {
                if let Some(e) = st.pipeline.stack.get_mut(i) {
                    e.enabled = v;
                }
                structural = true;
            }
            StackOp::SetMix(i, v) => {
                if let Some(e) = st.pipeline.stack.get_mut(i) {
                    e.mix = v.clamp(0.0, 1.0);
                }
                structural = true;
            }
        }
    }
    if structural {
        st.push_undo();
        st.after_change();
    }
}
