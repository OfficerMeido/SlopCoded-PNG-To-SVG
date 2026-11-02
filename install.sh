#!/usr/bin/env bash
# Installs dist/png_to_svg and adds "Convert PNG to SVG" to the Open With menu
# for PNG files. Per-user install, no sudo needed.
set -euo pipefail
cd "$(dirname "$0")"

BIN_SRC="dist/png_to_svg"
[ -x "$BIN_SRC" ] || { echo "Run ./build.sh first ($BIN_SRC not found)." >&2; exit 1; }

BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
mkdir -p "$BIN_DIR" "$APP_DIR"

install -m 755 "$BIN_SRC" "$BIN_DIR/png_to_svg"

cat > "$APP_DIR/png_to_svg.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Convert PNG to SVG
Comment=Vectorize PNG images to SVG next to the original
Exec=$BIN_DIR/png_to_svg %F
Icon=image-x-generic
Terminal=false
NoDisplay=false
MimeType=image/png;
Categories=Graphics;
DESKTOP

command -v update-desktop-database >/dev/null && update-desktop-database "$APP_DIR" || true

echo "Installed to $BIN_DIR/png_to_svg"
echo "Right-click a PNG > Open With > Convert PNG to SVG"
