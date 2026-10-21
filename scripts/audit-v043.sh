#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
printf 'zSoft Manager pre-release audit (no package updates or AppImage build)\n'
printf 'Project: %s\n' "$PWD"
for f in src/node.rs src/node_ui.rs src/python_env.rs src/python_ui.rs src/update.rs assets/node-release-schedule.json; do
  test -f "$f" || { printf 'Missing: %s\n' "$f" >&2; exit 1; }
done
if ! command -v cargo >/dev/null; then
  echo 'Cargo is missing. Cannot run Rust checks.' >&2
  exit 1
fi
cargo fmt --check
cargo test
cargo check --release
printf '\nAudit compilation and unit tests passed. Complete manual GUI tests in AUDIT-v0.4.3.md.\n'
