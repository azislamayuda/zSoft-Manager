# zSoft Manager

**zSoft Manager** is a lightweight, native Rust + GTK4 application manager for Ubuntu and related Linux distributions, created by [@azislamayuda](https://github.com/azislamayuda).

> Currently the managed application format is **AppImage**. The general name prepares the project for additional application formats in future versions; `.deb`, Flatpak and Snap support are **not yet implemented**.

## Features

- Native GTK4 flat soft-blue interface; no Electron, WebView, idle daemon, or automatic network polling.
- On-demand recursive AppImage discovery in **subfolders** of `~/Applications` only. Direct files in `~/Applications` and every `ArtCraft` directory are excluded.
- GitHub Releases update sources and the official KDE Kdenlive download server.
- Versionless stable filenames, compatibility symlinks, one-step backup/rollback and size / ELF / publisher SHA256 verification when available.
- Inline toast notifications for successful saves, scans, updates, failures and rollbacks; no extra windows.
- In-window About and Changelog pages.
- Header **Update** icon next to Changelog checks [this project's GitHub Releases](https://github.com/azislamayuda/zSoft-Manager/releases) and, after a second click, installs a verified update when running as an AppImage from a managed subfolder.
- Friendly errors when no release is published or running from `cargo run`.

## Requirements

Ubuntu with GTK4, Rust stable (Rust >= 1.83), `curl`, `git`, `pkg-config`, and GTK4 development headers:

```bash
sudo apt install -y build-essential pkg-config libgtk-4-dev curl git
cargo test
cargo build --release
cargo run --release
```

## AppImage installation and updates

Download the latest `zSoft-Manager-vX.Y.Z-x86_64.AppImage` from [GitHub Releases](https://github.com/azislamayuda/zSoft-Manager/releases) once a release exists, then:

```bash
mkdir -p "$HOME/Applications/zSoft-Manager"
install -m755 ~/Downloads/zSoft-Manager-v0.3.0-x86_64.AppImage   "$HOME/Applications/zSoft-Manager/zSoft-Manager.AppImage"
./scripts/install-desktop.sh
```

The example filename above refers to v0.3.0; adjust to the release you downloaded. The stable destination name is `zSoft-Manager.AppImage`. The application can update itself only from this managed subdirectory (not from `cargo run`, nor directly from the `~/Applications` root). The first click on the Update icon checks, and the second click confirms installation; restart after a successful self-update.

## Build an AppImage

```bash
cargo test
cargo build --release
bash scripts/build-appimage.sh
```

On x86_64 Ubuntu, the script downloads an official linuxdeploy binary if not already cached, assembles GTK4-linked components into an AppDir and writes the AppImage + SHA256 file to `dist/`. Runtime portability must still be smoke-tested on the target Ubuntu versions. It is also possible to create a release via the GitHub Actions workflow by pushing a tag, after building and testing locally.

```bash
git tag v0.3.0
git push origin main --tags
```

The GitHub Actions `release-appimage.yml` job will test, build, package, and upload the AppImage to GitHub Releases. A Release is required before the self-updater works; an empty repository is not a Release.

## Migrate from zAppImage Manager

The old project has local Git history; the target is a separate GitHub clone. To preserve that history and the cloned repository's `origin`, run:

```bash
bash scripts/migrate-to-github-clone.sh
```

This script imports historical Git commits into the target clone and retains the original project as a backup. Extract the **v0.3.0 patch ZIP** into the new clone afterwards. Source configuration reads existing `~/.config/zappimage-manager/sources.json` and state `~/.local/state/zappimage-manager/installed.json` as a fallback, and saves future changes in `~/.config/zsoft-manager` and `~/.local/state/zsoft-manager`. The old files are left untouched. The existing `.zappimage-manager.backup` disk naming is intentionally retained for rollback compatibility.

## UI status messages

Examples: `Changes saved successfully`, `Scan complete`, `Update finished`, `Rollback finished`, `Self-update failed`. Toast notifications disappear after 5 seconds. Check details in the per-app status and footer.

## License

Copyright (c) 2026 Azis Lamayuda. **Personal and non-commercial use only. No sale or commercial redistribution.** Source is publicly viewable and modifiable on those terms. See [LICENSE](LICENSE).

**Important:** this is a **source-available license**, not an OSI-approved open-source license. Preexisting releases of zAppImage Manager originally licensed MIT remain covered by their original license; existing rights are not revoked.

## Project layout

- `src/ui.rs` — GTK4 screens, toast notifications, actions
- `src/config.rs` — sources / version state with legacy fallback
- `src/update.rs` — GitHub + KDE downloads, validation, rollback
- `src/self_update.rs` — own GitHub Release check + guarded update
- `src/naming.rs`, `src/scan.rs` — stable filename and scan rules
- `scripts/build-appimage.sh` — AppImage builder
- `.github/workflows/release-appimage.yml` — CI build / release

Please use the issue tracker for feedback and bug reports.
