# Node.js old-version cleanup — v0.4.5

This pre-release improvement adds an optional **Remove old** action next to superseded NVM-managed Node.js installations. No cleanup happens automatically.

Example: If `v26.12.1`, `v26.12.0`, `v24.22.1` (default), `v24.22.0` are installed, the older `.0` patch versions can show **Remove old**. The latest of each major and default/active versions do not offer removal.

When confirmed the subprocess rechecks installed versions, active/default, and uses `nvm uninstall` only for the specifically selected version. Current-version upgrades within the same major now migrate global npm packages first with `--reinstall-packages-from`, as the older `znode.sh` did. Version-specific global npm packages are removed by NVM when its old version is uninstalled; review them before confirming. Existing open terminal sessions are not altered but may need to select a remaining version.

## Ubuntu checks

```bash
cargo fmt
bash scripts/audit-v045.sh
cargo run --release
```

On Node.js page: inspect Installed Node.js versions, verify candidate buttons for older same-major versions, click **Remove old** only if you wish, choose Cancel to test safe cancellation first, then (optionally) confirm one safe candidate. Verify `nvm ls`, `nvm alias default` and `nvm current` in terminal.

No AppImage build, commit, release or automatic npm package migration is part of this patch. Source code was statically inspected; Rust/GTK compilation requires Ubuntu.
