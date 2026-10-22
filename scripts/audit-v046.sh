#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
echo "[1/3] Rust formatting"
cargo fmt --check
echo "[2/3] Rust tests"
cargo test
echo "[3/3] Release type-check"
cargo check --release
echo "Rust checks passed. Next: verify summary toasts and Flatpak layout on Ubuntu."
