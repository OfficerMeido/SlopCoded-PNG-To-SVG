#!/usr/bin/env bash
# Builds a single self-contained Linux executable: dist/png_to_svg
# Needs: Python 3 with venv, and internet access for pip.
# Use a specific Python with:  PYTHON=python3.13 bash build.sh
set -euo pipefail
cd "$(dirname "$0")"

PYTHON="${PYTHON:-python3}"
echo "Using $($PYTHON --version)"

rm -rf .build-venv
"$PYTHON" -m venv .build-venv
source .build-venv/bin/activate
pip install --upgrade pip
pip install vtracer pillow pyinstaller

pyinstaller --onefile --clean --name png_to_svg png_to_svg.py

deactivate
echo
echo "Built: $(pwd)/dist/png_to_svg"
echo "Next:  bash install.sh"
