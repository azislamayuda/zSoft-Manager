# zSoft Manager v0.4.7 — Development, Testing & Release

New in v0.4.7: dedicated APT Manager (after Flatpak), read-only package previews, privileged operations through pkexec/Polkit, protected Autoremove, progress log, check-summary toast, and reboot flag.

## Apply patch (existing v0.4.6 checkout)

```bash
cd "$HOME/ZDATA/ME/GITHUB/zSoft-Manager"
unzip -o ~/Downloads/zSoft-Manager-v0.4.7-PATCH.zip -d .
```

The patch is based on v0.4.6 with the GitHub HTTP fix integrated. Back up any newer changes to overlapping source files before replacing them. Do not modify `.git`.

## Compile and audit

```bash
cargo fmt
bash scripts/audit-v047.sh
cargo run --release
```

See `APT-v0.4.7-TEST-NOTES.md` for read-only testing, confirmations, authorization, error handling and safety limits. Do **not** start by testing Autoremove on real packages.

## Package only after GUI/Polkit tests pass

```bash
bash scripts/test-and-build.sh
ls -lh dist/zSoft-Manager-v0.4.7-x86_64.AppImage*
cd dist
sha256sum -c zSoft-Manager-v0.4.7-x86_64.AppImage.sha256
./zSoft-Manager-v0.4.7-x86_64.AppImage
```

Upload the AppImage and matching `.sha256` sidecar to the GitHub release only after checking the packaged app on Ubuntu. Use GitHub Desktop for Commit/Push.

Note: pkexec is external OS functionality; AppImage packaging does not install polkit, policy rules, or admin helpers. A functioning system Polkit authentication agent is required for privileged APT operations.
