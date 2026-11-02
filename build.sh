#!/usr/bin/env bash
# Builds an optimized binary at target/release/png_to_svg.
# Needs the Rust toolchain (https://rustup.rs) and internet access for crates.
#
# Fully static build (runs on any Linux, no glibc version issues):
#   rustup target add x86_64-unknown-linux-musl
#   STATIC=1 bash build.sh        # binary: target/x86_64-unknown-linux-musl/release/png_to_svg
set -euo pipefail
cd "$(dirname "$0")"

command -v cargo >/dev/null || { echo "cargo not found. Install Rust from https://rustup.rs" >&2; exit 1; }

if [ "${STATIC:-0}" = "1" ]; then
    cargo build --release --target x86_64-unknown-linux-musl
    mkdir -p target/release
    cp target/x86_64-unknown-linux-musl/release/png_to_svg target/release/png_to_svg
else
    cargo build --release
fi

echo
echo "Built: $(pwd)/target/release/png_to_svg"
echo "Next:  bash install.sh"
