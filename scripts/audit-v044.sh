#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
printf 'zSoft Manager v0.4.4 pre-release audit (no package updates or AppImage build)\n'
printf 'Project: %s\n' "$PWD"
for f in src/node.rs src/node_ui.rs src/python_env.rs src/python_ui.rs src/update.rs assets/node-release-schedule.json; do
  test -f "$f" || { printf 'Missing: %s\n' "$f" >&2; exit 1; }
done
if ! command -v cargo >/dev/null; then
  echo 'Cargo is missing. Cannot run Rust checks.' >&2
  exit 1
fi
test -f tests/fixtures/github-appimage-regressions.json
cargo fmt --check
cargo test
cargo check --release
printf '\nRust checks passed. Next: GUI Check updates and optional live source audit.\n'
