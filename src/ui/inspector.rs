//! Правая панель — инспектор параметров выбранного эффекта.
//!
//! Показывает имя/группу/подсказку эффекта, его вкл/микс и ползунки по
//! `ParamSpec`. Значения пишутся прямо в `StackEntry.values`; после правок
//! зовём `after_change()`, чтобы пересобрать превью.

use std::sync::Arc;

use super::AppState;
use crate::theme::Palette;

/// Отрисовка панели.
pub fn show(ui: &mut egui::Ui, st: &mut AppState) {
    let p = st.palette.clone();

    let Some(idx) = st.selected else {
        placeholder(ui, &p, "Выбери эффект в стеке");
        return;
    };
    if idx >= st.pipeline.stack.len() {
        placeholder(ui, &p, "Выбери эффект в стеке");
        return;
    }

    let registry = Arc::clone(&st.registry);
    let id = st.pipeline.stack[idx].id.clone();
    let Ok(eff) = registry.get(&id) else {
        placeholder(ui, &p, "Эффект не найден");
        return;
    };

    let specs = eff.params();
    let name = eff.name();
    let group = eff.group().label();
    let hint = eff.hint();

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(name)
                .strong()
                .size(15.0)
                .color(p.on_surface),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(group)
                    .small()
                    .color(p.on_surface_variant),
            );
        });
    });
    if !hint.is_empty() {
        ui.label(
            egui::RichText::new(hint)
                .small()
                .italics()
                .color(p.on_surface_variant),
        );
    }
    ui.separator();

    let mut changed = false;
    let mut reset = false;
    {
        let entry = &mut st.pipeline.stack[idx];
        ui.horizontal(|ui| {
            if ui.checkbox(&mut entry.enabled, "Включён").changed() {
                changed = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Сбросить").clicked() {
                    reset = true;
                }
            });
        });
        if ui
            .add(egui::Slider::new(&mut entry.mix, 0.0..=1.0).text("Микс"))
            .changed()
        {
            changed = true;
        }
        ui.separator();

        if specs.is_empty() {
            ui.label(
                egui::RichText::new("У этого эффекта нет настроек").color(p.on_surface_variant),
            );
        } else {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (i, spec) in specs.iter().enumerate() {
                        let mut v = entry.values.get(i);
                        let mut slider =
                            egui::Slider::new(&mut v, spec.min..=spec.max).text(spec.label);
                        if spec.step > 0.0 {
                            slider = slider.step_by(spec.step as f64);
                        }
                        if ui.add(slider).changed() {
                            entry.values.set(i, v.clamp(spec.min, spec.max));
                            changed = true;
                        }
                        ui.label(
                            egui::RichText::new(spec.format(entry.values.get(i)))
                                .small()
                                .monospace()
                                .color(p.on_surface_variant),
                        );
                    }
                });
        }
    }

    if reset {
        let defaults = st.registry.get(&id).ok().map(|e| e.defaults());
        if let Some(d) = defaults {
            if let Some(entry) = st.pipeline.stack.get_mut(idx) {
                entry.values = d;
            }
        }
        st.push_undo();
        st.after_change();
    } else if changed {
        st.after_change();
    }
}

fn placeholder(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(text).color(p.on_surface_variant));
    });
}
