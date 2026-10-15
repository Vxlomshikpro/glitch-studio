#!/usr/bin/env bash
# Glitch Studio — сборка при необходимости и запуск окна.
#
# Скрипт сам разбирается, откуда его запустили:
#   • из терминала внутри Hyprland — просто собирает и запускает;
#   • из tty или от root — находит твою графическую сессию и перезапускает
#     приложение от твоего пользователя с правильным окружением.
set -euo pipefail

PROJECT="$(dirname "$(readlink -f "$0")")"
cd "$PROJECT"

BIN="target/release/glitch-studio"
DEBUG_BIN="target/debug/glitch-studio"
JOBS="${GLITCH_BUILD_JOBS:-2}"
OWNER="$(stat -c %U "$PROJECT")"

say()  { printf '\033[38;5;213m%s\033[0m\n' "$*"; }
err()  { printf '\033[31m%s\033[0m\n' "$*" >&2; }
die()  { err "$*"; exit 1; }

[[ -n "${OWNER}" && "$OWNER" != "root" ]] || OWNER="vxlomshikpro"

# ---- Подхват графической сессии --------------------------------------------
# Нужны WAYLAND_DISPLAY, XDG_RUNTIME_DIR и DBUS: без них окно не откроется,
# а файловые диалоги будут молча падать.
HOME_DIR_OVERRIDE="/home/$OWNER"

# Перезапуск нужен в двух случаях: окна нет (tty) или мы root, а окно нужно
# открывать от пользователя, чья это сессия.
need_relaunch() {
    [[ -z "${WAYLAND_DISPLAY:-}" || "$(id -u)" == "0" ]]
}

have_user_session() {
    local uid runtime
    uid="$(id -u "$OWNER" 2>/dev/null || echo "")"
    [[ -n "$uid" && "$uid" != "0" ]] || return 1
    runtime="/run/user/$uid"
    [[ -S "$runtime/bus" ]] || return 1
    export XDG_RUNTIME_DIR="$runtime"
    export WAYLAND_DISPLAY="wayland-1"
    export DBUS_SESSION_BUS_ADDRESS="unix:path=$runtime/bus"
    export XDG_CURRENT_DESKTOP="Hyprland"
}

if [[ "${GLITCH_NO_REEXEC:-0}" != "1" ]] && need_relaunch && have_user_session; then
    say "Запускаю от $OWNER в его сессии Hyprland."
    exec setpriv --reuid="$OWNER" --regid="$(id -g "$OWNER")" --init-groups \
        env HOME="$HOME_DIR_OVERRIDE" \
            XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" \
            WAYLAND_DISPLAY="$WAYLAND_DISPLAY" \
            DBUS_SESSION_BUS_ADDRESS="$DBUS_SESSION_BUS_ADDRESS" \
            XDG_CURRENT_DESKTOP="$XDG_CURRENT_DESKTOP" \
            "$0" "$@"
fi

if [[ -z "${WAYLAND_DISPLAY:-}" && -z "${DISPLAY:-}" ]]; then
    die "Графический сеанс не найден. Запусти из терминала Hyprland или укажи WAYLAND_DISPLAY."
fi

# ---- Сборка ------------------------------------------------------------------
command -v cargo >/dev/null 2>&1 || die "Rust не найден: sudo apt install cargo rustc"

if [[ -x "$BIN" ]] && ! find src Cargo.toml -newer "$BIN" -print -quit 2>/dev/null | grep -q .; then
    say "Уже собрано, запускаю."
elif [[ "${1:-}" == "--debug" ]]; then
    say "Собираю debug…"
    cargo build -j "$JOBS" || die "Сборка провалилась"
    BIN="$DEBUG_BIN"
else
    say "Собираю release, первый раз это несколько минут…"
    cargo build --release -j "$JOBS" || die "Сборка провалилась"
fi

[[ -x "$BIN" ]] || die "Бинарь не найден: $BIN"

args=("$@")
[[ "${1:-}" == "--debug" ]] && args=("${args[@]:1}")

# Запоминаем воркспейс, на котором ты сейчас работаешь: новое окно должно
# появиться именно там, а не в отдельном пустом воркспейсе.
TARGET_WS=""
if command -v hyprctl >/dev/null 2>&1 && [[ -n "${WAYLAND_DISPLAY:-}" ]]; then
    TARGET_WS="$(hyprctl activeworkspace -j 2>/dev/null \
        | sed -n 's/.*"name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)"
fi

say "Запускаю Glitch Studio…"

if [[ -n "$TARGET_WS" ]]; then
    (
        # Ждём, пока окно появится, переносим на нужный воркспейс и убираем фокус
        # обратно — иначе переключение воркспейса увело бы и терминал.
        for _ in $(seq 1 15); do
            if hyprctl dispatch movetoworkspacesilent "$TARGET_WS" "class:Glitch Studio" \
                >/dev/null 2>&1; then
                hyprctl dispatch focuswindow "class:Glitch Studio" >/dev/null 2>&1
            fi
            sleep 1
            hyprctl clients -j 2>/dev/null | grep -q '"class"[[:space:]]*:[[:space:]]*"Glitch Studio"' || true
        done
    ) >/dev/null 2>&1 &
fi

exec "$BIN" "${args[@]}"