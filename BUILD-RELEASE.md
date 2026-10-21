# zSoft Manager v0.4.5 — Development, Testing & Release

This development snapshot adds Flatpak management on top of the verified v0.4.4 AppImage/Node/Python code. **Do not publish until Rust tests and GUI smoke tests pass on Ubuntu.**

## Apply patch to existing GitHub Desktop clone

```bash
cd "$HOME/ZDATA/ME/GITHUB/zSoft-Manager"
unzip -o ~/Downloads/zSoft-Manager-v0.4.5-PATCH.zip -d .
cargo fmt
bash scripts/audit-v045.sh
cargo run --release
```

Keep `.git` and your user configuration untouched. The patch contains only source/docs/scripts, no binaries.

## Flatpak test checklist

See `FLATPAK-v0.4.5-TEST-NOTES.md`. Confirm user+system installation listings, app/runtime tabs, remote comparison, and one selected update before testing Update all. Never run the GUI with `sudo`.

```bash
flatpak --version
flatpak list --app --columns=name,application,version,branch,origin,installation,active
flatpak remote-ls --app --updates
```

## Final AppImage packaging (only after test success)

```bash
bash scripts/test-and-build.sh
cd dist
sha256sum -c zSoft-Manager-v0.4.5-x86_64.AppImage.sha256
./zSoft-Manager-v0.4.5-x86_64.AppImage
```

Expected release assets:

- `dist/zSoft-Manager-v0.4.5-x86_64.AppImage`
- `dist/zSoft-Manager-v0.4.5-x86_64.AppImage.sha256`

Commit/push through GitHub Desktop. Publish GitHub tag `v0.4.5` only after the packaged AppImage works.
