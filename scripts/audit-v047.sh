#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
echo "[1/4] Rust formatting"
cargo fmt --check
echo "[2/4] Rust regression tests"
cargo test
echo "[3/4] Release type-check"
cargo check --release
echo "[4/4] Verify APT source guards"
grep -q '"--no-remove"' src/apt.rs
grep -q 'Apt,\|    Apt,' src/ui.rs
if awk '/#\[cfg\(test\)\]/{exit}{print}' src/apt.rs | grep -E 'full-upgrade|--force-yes|allow-unauthenticated|rm -f /var/lib/dpkg/lock'; then
    echo 'Review: suspicious APT argument detected in source' >&2
    exit 1
fi
echo "APT source checks passed. Next: test GTK4 UI, Polkit authorization, and real package operations on Ubuntu."
