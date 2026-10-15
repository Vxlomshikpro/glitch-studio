//! Горячие клавиши. Действия собираются в список и выполняются в update().

use super::{Action, AppState};

pub struct Binding {
    pub keys: &'static str,
    pub desc: &'static str,
}

pub const BINDINGS: &[Binding] = &[
    Binding {
        keys: "Ctrl+O",
        desc: "Открыть файл",
    },
    Binding {
        keys: "Ctrl+Shift+O",
        desc: "Добавить папку в библиотеку",
    },
    Binding {
        keys: "Ctrl+S",
        desc: "Быстрое сохранение",
    },
    Binding {
        keys: "Ctrl+Shift+S",
        desc: "Сохранить как…",
    },
    Binding {
        keys: "Ctrl+E",
        desc: "Экспорт видео",
    },
    Binding {
        keys: "Ctrl+Z",
        desc: "Отменить изменение настроек",
    },
    Binding {
        keys: "Ctrl+Shift+Z",
        desc: "Повторить изменение",
    },
    Binding {
        keys: "R",
        desc: "Случайные настройки",
    },
    Binding {
        keys: "Shift+R",
        desc: "Новый seed",
    },
    Binding {
        keys: "Space",
        desc: "Play / pause видео",
    },
    Binding {
        keys: "← / →",
        desc: "Шаг по кадрам",
    },
    Binding {
        keys: "B (удерживать)",
        desc: "Показать оригинал",
    },
    Binding {
        keys: "Ctrl+1…9",
        desc: "Загрузить пресет",
    },
    Binding {
        keys: "Ctrl+T",
        desc: "Следующая тема",
    },
    Binding {
        keys: "F11",
        desc: "Полноэкранный режим",
    },
    Binding {
        keys: "Tab",
        desc: "Скрыть боковые панели",
    },
    Binding {
        keys: "Ctrl+/",
        desc: "Список горячих клавиш",
    },
];

/// egui отдаёт клавиши вне раскладки, поэтому сравниваем `key`, а не текст.
/// `i.consume_key(e)` не даёт событию дойти до сфокусированного виджета.
fn press(ctx: &egui::Context, want: egui::Key, ctrl: bool, shift: bool) -> bool {
    let mut hit: Option<(egui::Modifiers, egui::Key)> = None;
    ctx.input(|i| {
        for e in &i.events {
            let egui::Event::Key {
                key,
                pressed,
                modifiers,
                ..
            } = e
            else {
                continue;
            };
            if !pressed || *key != want {
                continue;
            }
            if modifiers.ctrl == ctrl && modifiers.shift == shift && !modifiers.alt {
                hit = Some((*modifiers, want));
            }
        }
    });
    match hit {
        Some((m, k)) => ctx.input_mut(|i| i.consume_key(m, k)),
        None => false,
    }
}

pub fn collect(ctx: &egui::Context, st: &mut AppState) -> Vec<Action> {
    let mut out = Vec::new();

    let mut b_down = false;
    ctx.input(|i| {
        for e in &i.events {
            if let egui::Event::Key { key, pressed, .. } = e {
                if *key == egui::Key::B {
                    b_down = *pressed;
                }
            }
        }
    });
    st.show_original = b_down;

    if press(ctx, egui::Key::O, true, false) {
        open_file_dialog(st);
    }
    if press(ctx, egui::Key::O, true, true) {
        add_folder_dialog(st);
    }
    if press(ctx, egui::Key::S, true, false) {
        out.push(Action::Save);
    }
    if press(ctx, egui::Key::S, true, true) {
        out.push(Action::SaveAs);
    }
    if press(ctx, egui::Key::E, true, false) {
        out.push(Action::OpenExport);
    }
    if press(ctx, egui::Key::Z, true, false) {
        st.undo();
    }
    if press(ctx, egui::Key::Z, true, true) {
        st.redo();
    }
    if press(ctx, egui::Key::R, false, false) {
        out.push(Action::Randomize);
    }
    if press(ctx, egui::Key::R, false, true) {
        out.push(Action::NewSeed);
    }
    if press(ctx, egui::Key::T, true, false) {
        out.push(Action::NextTheme);
    }
    if press(ctx, egui::Key::Slash, true, false) {
        out.push(Action::OpenHotkeys);
    }
    if press(ctx, egui::Key::F11, false, false) {
        let current = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!current));
    }
    if press(ctx, egui::Key::Tab, false, false) {
        st.left_panel = !st.left_panel;
        st.right_panel = !st.right_panel;
    }
    if st.player.is_open() {
        if press(ctx, egui::Key::Space, false, false) {
            st.player.playing = !st.player.playing;
        }
        if press(ctx, egui::Key::ArrowLeft, false, false) {
            st.player.step(-1);
            st.request_preview(true);
        }
        if press(ctx, egui::Key::ArrowRight, false, false) {
            st.player.step(1);
            st.request_preview(true);
        }
    }

    if let Some(i) = preset_digit(ctx) {
        if i < st.presets.presets.len() {
            out.push(Action::LoadPreset(i));
        }
    }

    out
}

fn preset_digit(ctx: &egui::Context) -> Option<usize> {
    let mut hit = None;
    ctx.input(|i| {
        for e in &i.events {
            let egui::Event::Key {
                key,
                pressed,
                modifiers,
                ..
            } = e
            else {
                continue;
            };
            if !pressed || !modifiers.ctrl || modifiers.shift {
                continue;
            }
            let n = match key {
                egui::Key::Num1 => 1,
                egui::Key::Num2 => 2,
                egui::Key::Num3 => 3,
                egui::Key::Num4 => 4,
                egui::Key::Num5 => 5,
                egui::Key::Num6 => 6,
                egui::Key::Num7 => 7,
                egui::Key::Num8 => 8,
                egui::Key::Num9 => 9,
                _ => continue,
            };
            hit = Some((n - 1, *modifiers, *key));
        }
    });
    match hit {
        Some((n, m, k)) => {
            ctx.input_mut(|i| i.consume_key(m, k));
            Some(n)
        }
        None => None,
    }
}

fn open_file_dialog(st: &mut AppState) {
    let start = st
        .current
        .as_ref()
        .and_then(|p| p.parent().map(|x| x.to_path_buf()))
        .or_else(|| std::env::var("HOME").ok().map(std::path::PathBuf::from))
        .unwrap_or_default();
    if let Some(p) = rfd::FileDialog::new()
        .add_filter(
            "Медиа",
            &[
                "png", "jpg", "jpeg", "webp", "bmp", "gif", "mp4", "mkv", "webm", "mov",
            ],
        )
        .set_directory(start)
        .pick_file()
    {
        st.pending.push(Action::OpenFile(p));
    }
}

fn add_folder_dialog(st: &mut AppState) {
    let start = std::env::var("HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    if let Some(p) = rfd::FileDialog::new().set_directory(start).pick_folder() {
        st.pending.push(Action::AddFolder(p));
    }
}

pub fn show_window(ctx: &egui::Context, st: &mut AppState) {
    if !st.hotkeys_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Горячие клавиши")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let p = st.p();
            egui::Grid::new("keys")
                .num_columns(2)
                .spacing([24.0, 6.0])
                .show(ui, |ui| {
                    for b in BINDINGS {
                        ui.label(
                            egui::RichText::new(b.keys)
                                .monospace()
                                .strong()
                                .color(p.primary),
                        );
                        ui.label(b.desc);
                        ui.end_row();
                    }
                });
            ui.add_space(8.0);
            if ui.button("Закрыть").clicked() {
                st.hotkeys_open = false;
            }
        });
    if !open {
        st.hotkeys_open = false;
    }
}
