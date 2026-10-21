# zSoft Manager v0.4.3 — Pre-release audit

## Scope

This source candidate incorporates the E0505 Rust patch, npm JSON patch, Python GTK borrow fix, and a reduction in Node GTK list redraws. Version remains `0.4.3`. No AppImage binary is generated or released by this audit.

## Verified without installing or updating packages

- Existing Rust fixture tests cover AppImage scan exclusion, KDE stable-series parsing, GitHub release pending/checksum, backup/rollback, and version comparisons. Their presence was reviewed; they have **not** been executed here.
- Bash execution using mocked NVM/npm verifies valid JSON when `npm outdated` exits 0 with no packages or exits 1 with update results.
- Inspected `src/python_env.rs`: pip always runs as `<venv>/bin/python -m pip`; paths without `pyvenv.cfg` are rejected, and updates are limited to selected top-level packages. `pip check` is executed after success.
- Inspected GTK toast code: bottom-center alignment and sequence tokens prevent an older hide timer from hiding a newer toast. A five-second timer is scheduled for each toast, so extremely frequent notifications can still cause multiple short-lived timers.
- Inspected startup: Node/Python GTK views are lazy-loaded; AppImage paths are scanned at startup; `Rescan` reuses existing AppImage rows for identical paths.

## Must test on Ubuntu (not verified in this sandbox)

1. `cargo fmt && cargo fmt --check && cargo test && cargo build --release`
2. `cargo run --release`: verify GitHub self-update check, and force/retry when a release is missing its asset (without installation).
3. Node page > Refresh installed; Check updates with (a) outdated packages, (b) no outdated packages; verify neither reports invalid JSON. Test Set default only if you wish to change future shell settings.
4. Python page > Activate `~/myenv`, Refresh installed, Check updates; cancel or close before any package upgrade. Test invalid venv path, confirm it displays an error without a GTK panic. Backup the pip environment before clicking Update selected.
5. AppImage page > Rescan; verify ArtCraft/root exclusions. Check one GitHub-sourced app and Kdenlive. **Do not** click Install update until ready.
6. Trigger two toasts quickly and verify the newest message stays visible for its own timeout.
7. Compare RAM/RSS on the same Ubuntu session and idle conditions for v0.4.2.1 and v0.4.3. `ps -C zsoft-manager -o pid,rss,etime,args` reports RSS in KiB.

## Release gate

Do not publish v0.4.3 until Cargo tests, GUI smoke tests, and packaged AppImage startup all pass. Do not confuse a successful `cargo build` with a working portable AppImage.
