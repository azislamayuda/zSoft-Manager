# zSoft Manager v0.4.7 — Flatpak Compact Rows and Commit Tooltip

## Changes

- Flatpak Applications and Runtimes cards now display only a checkbox, application name and ID, and Installed / Latest versions plus meaningful update status.
- The architecture field is hidden from the visible card, not deleted from internal data or update safety checks.
- Hover over the installed/latest version label to see the architecture, scope, origin, branch, installed commit, and remote commit, if available.
- The existing `active_commit` metadata is now read by GTK tooltip generation. It is retained for Flatpak ref identity and diagnostics, eliminating the unused-field warning.
- The update confirmation, selection, permissions, and CLI backend have not been changed.

## Test in Ubuntu (VS Code terminal)

```bash
cd "$HOME/ZDATA/ME/GITHUB/zSoft-Manager"
cargo fmt
bash scripts/audit-v047.sh
cargo run --release
```

1. Navigate to Flatpak > Applications and check the two-column layout: `Name / com.example.App` and `Installed: ... · Latest: ...`. Architecture should not be visible.
2. Hover over the version text. Its tooltip should contain architecture and local commit (plus remote commit if a remote check was made).
3. Switch to Runtimes and confirm the same row format.
4. Click Check updates; check status and checkbox behavior; do not force package updates if none are pending.
5. Confirm `cargo test` and `cargo check --release` no longer warn that `active_commit` is never read.

### Expected limitations

The backend still requires architecture internally to identify valid Flatpak refs. Never remove `arch` from `src/flatpak.rs`.
