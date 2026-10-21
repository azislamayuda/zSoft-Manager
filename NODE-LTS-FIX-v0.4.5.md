# v0.4.5 — NVM old-LTS removal and visible errors

- When removing an old LTS release, ignore a stale version inherited through the parent terminal PATH **only after checking the configured NVM default and switching the isolated subprocess to that default**.
- Keep protections for default, selected active subprocess, no newer release in the same major, and version validation. Already-open terminals can still point at deleted versions, as warned in the confirmation dialog.
- Show a GTK error dialog when Node.js operations fail instead of only writing a message below the scroll area.
- Include stdout in error information when NVM exits nonzero with no stderr, capped to 1500 characters.

## Ubuntu verification

```bash
cargo fmt
bash scripts/audit-v045.sh
cargo run --release
```

First click Remove old on v24.22.0, choose Cancel, then confirm if you wish. Verify `nvm ls` and `nvm alias default`. Do not change the default or uninstall manually. Rebuild the AppImage only after successful validation.

The mock shell regression checks passed; Rust/GTK4 compilation is not available in this environment.
