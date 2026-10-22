> Historical v0.4.7 dashboard integration notes. For the latest compact card and 3/4-column compact layout, see `DASHBOARD-COMPACT-v0.4.7-TEST-NOTES.md`.

# zSoft Manager v0.4.7 — Dashboard & About integration

## Changes

1. Dashboard is the default landing page, with a navigation icon before AppImage.
2. Five GTK4 cards summarize installed local software: AppImage, Flatpak, APT, Node.js/npm, Python/pip. Counts and up to four item names/versions are shown with a shortcut to the respective manager.
3. All inventory collection is read-only in short-lived threads. No network update checks, Polkit, or APT modifications run automatically.
4. Flatpak apps and runtimes are separately counted; APT installed packages are not confused with graphical app counts. Failed local queries show **Inventory unavailable**.
5. About and Changelog are now separate cards on a single About navigation page. Both Back to applications and Network detail are removed.

## Apply only to existing v0.4.7 source

```bash
cd "$HOME/ZDATA/ME/GITHUB/zSoft-Manager"
unzip -o ~/Downloads/zSoft-Manager-v0.4.7-DASHBOARD-PATCH.zip -d .
cargo fmt
bash scripts/audit-v047.sh
cargo run --release
```

## Test checklist

- [ ] Dashboard is the initial page and the leftmost header icon is selected.
- [ ] All five cards display local counts/names and their **Open** buttons navigate correctly, including first-time lazy loading of Flatpak, APT, Node and Python.
- [ ] AppImage total matches the scanner (only managed subfolders; excludes ArtCraft and the root folder).
- [ ] Flatpak app/runtime counts match `flatpak list --app` and `flatpak list --runtime` across user/system installations.
- [ ] APT count reflects packages with dpkg installed state (`ii`), not the number of upgradeable packages.
- [ ] Node lists installed NVM versions and global npm packages; Python uses configured venv top-level pip packages.
- [ ] Refresh overview works; no network update operation, root prompt, or package modification happens.
- [ ] About card appears above a separate Changelog card. No Back to applications, extra Changelog icon, or Network detail.
- [ ] The original AppImage/Flatpak/APT/Node/Python pages still function.
- [ ] RAM and GUI responsiveness are acceptable; compare startup and post-navigation values before declaring an improvement.

No AppImage build or release is part of this source patch. Rust/GTK4 compilation must be verified on Ubuntu (toolchain unavailable in the artifact environment).
