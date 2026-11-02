pkgname=slopcoded-png-to-svg
_pkgname=SlopCoded-PNG-To-SVG
pkgver=0.1
pkgrel=1
pkgdesc="A Slop Coded PNG To SVG Converter"
arch=('x86_64')
url="https://github.com/OfficerMeido/SlopCoded-PNG-To-SVG"
license=('Unlicense')
depends=('python312')
provides=('png_to_svg')
conflicts=('png_to_svg')
options=('!strip')

source=(${_pkgname}::git+$url.git)
sha256sums=('SKIP')

prepare() {
  chmod +x ${srcdir}/${_pkgname}/"png_to_svg.py"
  PYTHON="${PYTHON:-python3.12}"
  echo "Using $($PYTHON --version)"
  "$PYTHON" -m venv ${srcdir}/${_pkgname}/.build-venv
  source ${srcdir}/${_pkgname}/.build-venv/bin/activate
  pip install --upgrade pip
  pip install vtracer pillow pyinstaller
}

package() {
  pyinstaller --onefile --clean --name ${srcdir}/${_pkgname}/png_to_svg ${srcdir}/${_pkgname}/png_to_svg.py
  BIN_SRC="${srcdir}/dist/"
  BIN_DIR="${HOME}/.local/bin"
  APP_DIR="${HOME}/.local/share/applications"
  mkdir -p "$BIN_DIR" "$APP_DIR"
  install -m 755 ${BIN_SRC}/png_to_svg "$BIN_DIR/png_to_svg"
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

}
