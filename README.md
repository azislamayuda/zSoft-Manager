# zSoft Manager

**zSoft Manager** is a lightweight, native Rust + GTK4 application manager for Ubuntu and related Linux distributions, created by [@azislamayuda](https://github.com/azislamayuda).

**Current release: v0.4.0** — AppImage updates and NVM-managed Node.js / npm updates. Updates run only when you request them.

> The app currently manages **AppImage** files and an **NVM-managed Node.js toolchain**. `.deb`, Flatpak and Snap package management are not yet implemented.

## Features

- Native GTK4 flat soft-blue interface; no Electron, WebView, idle daemon, or automatic network polling.
- On-demand recursive AppImage discovery in **subfolders** of `~/Applications` only. Direct files in `~/Applications` and every `ArtCraft` directory are excluded.
- GitHub Releases update sources and the official KDE Kdenlive download server.
- Versionless stable filenames, compatibility symlinks, one-step backup/rollback and size / ELF / publisher SHA256 verification when available.
- Inline toast notifications for successful saves, scans, updates, failures and rollbacks; no extra windows.
- Four in-window pages: **AppImage**, **Node.js**, **About**, and **Changelog**.
- **Node.js Toolchain:** inspect installed NVM, active Node.js, Node LTS and npm without network; manually check upstream versions; choose individual installs for NVM, Node LTS, npm and optional Node Current.
- Node LTS stays the default; no automatic deletion of old Node versions and no system Node.js packages or sudo operations.
- Only the current navigation icon has a soft-blue background; inactive icon-only buttons are transparent.
- GTK4 client-side titlebar with grouped, more compact minimize / maximize / close controls and an updated **Zs monogram** SVG icon.
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

## Node.js toolchain page

1. Click the **Node.js** icon in the header to see locally installed versions (no remote call).
2. Click **Check updates** to query official NVM Git tags, Node versions via NVM and npm metadata via npm.
3. Select a specific **Install update** button. zSoft Manager does not run all updates automatically.
4. After a successful update, choose **Check updates** again to verify the remote version.

Node.js installs are restricted to an existing NVM setup at `~/.nvm` (or `$NVM_DIR`). The NVM update button requires NVM itself to be a Git checkout. If NVM is missing or installed outside a Git checkout, the page explains this instead of executing a downloaded remote installation script. A system-level Node.js package managed by APT is never overwritten.

**Safety:** Previous Node versions are never uninstalled automatically. The optional Node Current installer does not change the default LTS alias. `npm` updates use `nvm install-latest-npm` so the version selected is compatible with the active LTS. The npm registry's latest published version may therefore differ from the installed compatible version.

## AppImage installation and updates

Download the latest `zSoft-Manager-vX.Y.Z-x86_64.AppImage` from [GitHub Releases](https://github.com/azislamayuda/zSoft-Manager/releases) once a release exists, then:

```bash
mkdir -p "$HOME/Applications/zSoft-Manager"
install -m755 ~/Downloads/zSoft-Manager-v0.4.0-x86_64.AppImage   "$HOME/Applications/zSoft-Manager/zSoft-Manager.AppImage"
./scripts/install-desktop.sh
```

The example filename above refers to v0.4.0; adjust to the release you downloaded. The stable destination name is `zSoft-Manager.AppImage`. The application can update itself only from this managed subdirectory (not from `cargo run`, nor directly from the `~/Applications` root). The first click on the Update icon checks, and the second click confirms installation; restart after a successful self-update.

## Build an AppImage

```bash
cargo test
cargo build --release
bash scripts/build-appimage.sh
```

On x86_64 Ubuntu, the script downloads an official linuxdeploy binary if not already cached, assembles GTK4-linked components into an AppDir and writes the AppImage + SHA256 file to `dist/`. Runtime portability must still be smoke-tested on the target Ubuntu versions. It is also possible to create a release via the GitHub Actions workflow by pushing a tag, after building and testing locally.

Use GitHub Desktop for **Commit to main** and **Push origin**. Create the tag `v0.4.0` from GitHub's new-release page, rather than using Git CLI.

The GitHub Actions `release-appimage.yml` job will test, build, package, and upload the AppImage to GitHub Releases. A Release is required before the self-updater works; an empty repository is not a Release.

## GitHub Desktop workflow

The development repository is expected at `/home/lamayuda/ZDATA/ME/GITHUB/zSoft-Manager`. Open it in GitHub Desktop, review the changed files, click **Commit to main**, then **Push origin**. Do not copy or overwrite `.git` from a source bundle. `target/`, `build/`, and `dist/` are generated locally and excluded from Git.

## Previous zAppImage Manager migration

If you already cloned `zSoft-Manager` via GitHub Desktop, **do not run the old migration script again**. Extract the v0.4.0 PATCH into your existing clone. Do not overwrite `.git`. Local Git operations are performed with GitHub Desktop, not the command line.

Source configuration reads existing `~/.config/zappimage-manager/sources.json` and state `~/.local/state/zappimage-manager/installed.json` as a fallback, and saves future changes under `zsoft-manager`. Old files remain untouched and existing rollback backups are preserved.

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
- `src/node.rs`, `src/node_ui.rs` — native Node toolchain backend and flat GTK4 page
- `src/naming.rs`, `src/scan.rs` — stable filename and scan rules
- `scripts/build-appimage.sh` — AppImage builder
- `.github/workflows/release-appimage.yml` — CI build / release

Please use the issue tracker for feedback and bug reports.
