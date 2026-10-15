#!/usr/bin/env bash
# Сборка AppImage (x86_64) и, если доступен кросс-компилятор, aarch64.
#
# AppImage = ELF-рантайм + squashfs с AppDir. Собирается вручную, без
# linuxdeploy: зависимости подтягиваются через ldd, а не через скрипт.
set -euo pipefail

ROOT="$(dirname "$(dirname "$(readlink -f "$0")")")"
cd "$ROOT"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
ARCH="${ARCH:-x86_64}"
OUT="$ROOT/dist"
TOOLS="${GLITCH_TOOLS:-/tmp/opencode/tools}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

say() { printf '\033[38;5;213m%s\033[0m\n' "$*"; }
die() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null || die "Rust не найден"
command -v mksquashfs >/dev/null || die "Нужен mksquashfs: sudo apt install squashfs-tools"

APPIMAGETOOL="$TOOLS/appimagetool-x86_64.AppImage"
[[ -f "$APPIMAGETOOL" ]] || die "Нет appimagetool в $TOOLS (скачай AppImageKit-x86_64.AppImage с GitHub/AppImage/AppImageKit)"

# На машине нет FUSE, поэтому AppImage-тул не запускается как есть. Распаковываем
# его и зовём бинарник напрямую, подсовывая библиотеки из его же AppDir —
# они собираны под libgpgme.so.11, которой в Kali нет.
EXTRACT_DIR="$TOOLS/squashfs-root"
if [[ ! -x "$EXTRACT_DIR/usr/bin/appimagetool" ]]; then
    say "Распаковываю appimagetool…"
    ( cd "$TOOLS" && chmod +x "$APPIMAGETOOL" && "$APPIMAGETOOL" --appimage-extract >/dev/null )
fi
[[ -x "$EXTRACT_DIR/usr/bin/appimagetool" ]] || die "appimagetool не распаковался"
APPIMAGETOOL_BIN="$EXTRACT_DIR/usr/bin/appimagetool"
APPIMAGETOOL_LD="$EXTRACT_DIR/usr/lib:$EXTRACT_DIR/usr/lib/x86_64-linux-gnu"

APPDIR="$ROOT/build/AppDir-$ARCH"
rm -rf "$ROOT/build" "$APPDIR"
mkdir -p "$APPDIR" "$OUT"

# ---- Бинарь ------------------------------------------------------------------
if [[ "$ARCH" != "x86_64" ]]; then
    command -v cargo-zigbuild >/dev/null || die "Для $ARCH нужен cargo-zigbuild: cargo install cargo-zigbuild"
    say "Кросс-сборка $ARCH…"
    cargo zigbuild --release --target "${ARCH}-unknown-linux-gnu" || die "Кросс-сборка не удалась"
    BIN_SRC="$TARGET_DIR/${ARCH}-unknown-linux-gnu/release/glitch-studio"
else
    say "Сборка release для x86_64…"
    cargo build --release -j "${GLITCH_BUILD_JOBS:-2}" || die "Сборка не удалась"
    BIN_SRC="$TARGET_DIR/release/glitch-studio"
fi
[[ -f "$BIN_SRC" ]] || die "Бинарь не найден: $BIN_SRC"

install -Dm755 "$BIN_SRC" "$APPDIR/usr/bin/glitch-studio"
install -Dm644 assets/icons/256x256.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/glitch-studio.png"
# appimagetool ищем иконку в корне AppDir — без неё он отказывается собирать образ.
install -Dm644 assets/icons/256x256.png "$APPDIR/glitch-studio.png"
install -Dm644 assets/icons/128x128.png "$APPDIR/usr/share/icons/hicolor/128x128/apps/glitch-studio.png"
install -Dm644 assets/icons/64x64.png   "$APPDIR/usr/share/icons/hicolor/64x64/apps/glitch-studio.png"
install -Dm644 README.md "$APPDIR/usr/share/doc/glitch-studio/README.md"

# ---- Метаданные --------------------------------------------------------------
cat > "$APPDIR/glitch-studio.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Glitch Studio
GenericName=Порча фото и видео
Comment=Намеренная порча фото и видео в стиле glitchcore
Exec=glitch-studio %F
Icon=glitch-studio
Terminal=false
Categories=Graphics;2DGraphics;AudioVideo;
StartupWMClass=Glitch Studio
X-AppImage-Version=$VERSION
EOF

# ---- AppRun (финальная версия после подбора библиотек) --------------------
# shellcheck disable=SC2016
# ---- Библиотеки --------------------------------------------------------------
# Собираем только те .so, что реально нужны бинарю: тянуть весь графический
# стек (mesa, vulkan, tbb) весит сотни мегабайт и ломает запуск на месте.
for soname in $(objdump -p "$BIN_SRC" 2>/dev/null \
                 | awk '/NEEDED/ {print $2}' \
                 | grep -E '^lib(gtk|gdk|glib|gl|EGL|X11|wayland|xkb|pango|cairo|gio|dbus)' \
                 | sort -u); do
    full="$(ldconfig -p 2>/dev/null | awk -v s="$soname" '$1==s {print $NF; exit}')"
    [[ -n "$full" && -f "$full" ]] || continue
    install -Dm755 "$full" "$APPDIR/usr/lib/$soname"
done

cat > "$APPDIR/AppRun" <<EOF
#!/bin/sh
HERE="\$(dirname "\$(readlink -f "\$0")")"
export PATH="\$HERE/usr/bin:\$PATH"
export LD_LIBRARY_PATH="\$HERE/usr/lib:\${LD_LIBRARY_PATH:-}"
if [ -z "\$DBUS_SESSION_BUS_ADDRESS" ] && [ -S "\${XDG_RUNTIME_DIR:-/run/user/\$(id -u)}/bus" ]; then
    export DBUS_SESSION_BUS_ADDRESS="unix:path=\${XDG_RUNTIME_DIR:-/run/user/\$(id -u)}/bus"
fi
exec "\$HERE/usr/bin/glitch-studio" "\$@"
EOF
chmod +x "$APPDIR/AppRun"

say "Собираю AppImage $VERSION ($ARCH)…"
LD_LIBRARY_PATH="$APPIMAGETOOL_LD" ARCH="$ARCH" "$APPIMAGETOOL_BIN" "$APPDIR" "$OUT/glitch-studio-$VERSION-$ARCH.AppImage" || die "appimagetool не справился"

chmod +x "$OUT/glitch-studio-$VERSION-$ARCH.AppImage"
say "Готово: $OUT/glitch-studio-$VERSION-$ARCH.AppImage"
ls -lh "$OUT"/*.AppImage