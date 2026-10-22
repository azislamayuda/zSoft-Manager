# Flatpak Manager v0.4.5 — Ubuntu test plan

1. `cargo fmt && bash scripts/audit-v045.sh` (Rust toolchain + GTK4 headers required).
2. `cargo run --release`, confirm the Flatpak icon is second in the header after AppImage.
3. Flatpak page shows **Applications** by default. Confirm installed apps, versions, origin, scope. Click **Runtimes** and check runtime list.
4. Click **Rescan** (local-only). Click **Check updates** (remote). Verify update flags against `flatpak remote-ls --updates` and same-scope origin.
5. Select one genuinely outdated application and click **Update selected**. Confirm it becomes Up to date after automatic refresh. No update should occur before clicking.
6. When additional updates exist, test Update all on the current tab. Runtime updates are managed separately.
7. Verify system-wide updates request authorization as needed, without running zSoft Manager as root.
8. Test offline/network error and missing CLI: page must show a warning, not freeze or claim Up to date.
9. Confirm AppImage, Node.js, Python, About, Changelog and self-update still work unchanged.
10. Build final AppImage only after Ubuntu tests and GUI tests pass.

## Notes
- Version metadata can be stale/empty; Flatpak update availability is determined by ref/commit.
- `--user` and default `--system` supported; custom named installation requires future support.
- `flatpak remote-ls` updates depend on upstream metadata and remote availability.
- For authorization errors, inspect the Flatpak/Polkit service rather than using `sudo zsoft-manager`.
- Offline source validation in this package does **not** constitute a successful Cargo/GTK compilation.
