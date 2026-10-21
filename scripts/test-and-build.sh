#!/usr/bin/env bash
set -euo pipefail
project="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project"
command -v cargo >/dev/null || { echo 'Cargo/Rust is not installed.' >&2; exit 1; }
command -v pkg-config >/dev/null || { echo 'pkg-config is missing.' >&2; exit 1; }
pkg-config --exists gtk4 || { echo 'GTK4 development headers missing: install libgtk-4-dev.' >&2; exit 1; }
cargo fmt
cargo fmt --check
cargo test
cargo build --release
bash scripts/build-appimage.sh
version="$(sed -n '/^version *=/{s/.*= *"\([^" ]*\)".*/\1/p;q;}' Cargo.toml)"
release_version="${version//+/.}"
(cd dist && sha256sum -c "zSoft-Manager-v${release_version}-x86_64.AppImage.sha256")
printf '\nBuild complete. Test the AppImage in dist/ before uploading it to GitHub Releases.\n'
