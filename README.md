# zSoft Manager

**zSoft Manager** is a lightweight, native Rust + GTK4 application manager for Ubuntu and related Linux distributions, created by [@azislamayuda](https://github.com/azislamayuda).

**Current development version: v0.4.7** — AppImage, Flatpak, NVM-managed Node.js, and Python virtual environment updates. Network checks run only on request.

> The app manages **AppImage**, **Flatpak** (applications and runtimes), **NVM-managed Node.js**, and **Python virtual environment** packages. `.deb` and Snap package management are not implemented.

## Features

- Native GTK4 flat soft-blue interface; no Electron, WebView, idle daemon, or automatic network polling.
- On-demand recursive AppImage discovery in **subfolders** of `~/Applications` only. Direct files in `~/Applications` and every `ArtCraft` directory are excluded.
- GitHub Releases update sources and the official KDE Kdenlive download server.
- Versionless stable filenames, compatibility symlinks, one-step backup/rollback and size / ELF / publisher SHA256 verification when available.
- Bottom-center in-window toast notifications for successful saves, scans, Flatpak checks/updates, failures and rollbacks; no extra windows.
- Six in-window pages: **AppImage**, **Flatpak**, **Node.js**, **Python**, **About**, and **Changelog**.
- **Node.js Toolchain:** inspect NVM, active/default Node, lifecycle status, all installed versions and global npm packages locally; manually check upstream versions and install individual updates.
- NVM default can be switched explicitly; no automatic deletion of old Node versions, system Node changes, or sudo operations.
- **Python Environment:** activate/deactivate an environment for in-app operations; inspect Python/pip and top-level packages; manually check `pip list --outdated --not-required`; select packages to upgrade and run `pip check`.
- Default Python virtual environment: `~/myenv` (configurable, including `/home/myenv`); zSoft Manager does not modify other terminals or the system Python installation.
- All seven header navigation icons use consistent 20 px GTK symbolic line art with 44 px hit targets. Only the active page gets a soft-blue background; inactive buttons remain transparent.
- GTK4 client-side titlebar with grouped, more compact minimize / maximize / close controls and an updated **Zs monogram** SVG icon.
- Default window size 1040 × 780 pixels (was 1040 × 720).
- Header **Update** icon next to Changelog checks [this project's GitHub Releases](https://github.com/azislamayuda/zSoft-Manager/releases) and, after a second click, installs a verified update when running as an AppImage from a managed subfolder.
- Friendly errors when no release is published or running from `cargo run`.

## Python virtual environment (v0.4.1)

The Python icon sits immediately after the Node.js icon. To use it:

1. Enter your virtual environment directory. The default is `~/myenv`, matching the previous `mpip` / `zpip` aliases. You may enter `/home/myenv` if that is your actual location.
2. Click **Activate**. This only selects the environment for zSoft Manager's commands; it does not run `source .../bin/activate` or alter existing terminal sessions.
3. Click **Check updates** for a PyPI query equivalent to `~/myenv/bin/python -m pip list --outdated --not-required --format=json`.
4. Uncheck packages you do not want to upgrade, then click **Update selected**. The app runs the virtual environment's `python -m pip install --upgrade ...`, followed by `python -m pip check`.
5. Click **Deactivate** to stop Python management in this GUI session.

Installed packages are never upgraded without clicking **Update selected**. pip installations run without a shell and without sudo; zSoft Manager does not update Python itself or packages outside the chosen virtual environment. pip may also update or install transitive dependencies when required by the selected top-level packages. Before upgrading complex environments, back up your dependencies with `~/myenv/bin/python -m pip freeze > requirements-before-update.txt`.

## Bootstrap Studio and GitHub Copilot update sources

- **Bootstrap Studio** uses its official, license-aware, built-in update system. Its download link comes from the license email; it is not an ordinary public GitHub Release, so zSoft Manager does not try to bypass the built-in updater. Official help: https://bootstrapstudio.io/contact-us
- **GitHub Copilot desktop application (Linux AppImage)** publishes builds at https://github.com/github/app/releases . Configure the AppImage Source provider as GitHub, repository `github/app`, release asset `GitHub-Copilot-linux-x64.AppImage`. Set the installed pattern and directory to match your actual `~/Applications` layout. See `examples/sources.example.json`.
- **GitHub Copilot CLI** uses a *different* repository: https://github.com/github/copilot-cli/releases . The **VS Code extension** updates through VS Code's extension manager, not AppImage.

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

**Safety:** Previous Node versions are never uninstalled automatically. The optional Node Current installer preserves whichever default alias you selected. `npm` updates use `nvm install-latest-npm` so the version selected is compatible with the active LTS. The npm registry's latest published version may therefore differ from the installed compatible version.

## Node.js improvements (v0.4.2)

- **Active Node badge:** displays `LTS`, `Current`, `EOL`, or `Unknown`; the tooltip distinguishes Active LTS and Maintenance LTS and includes the codename when published. The app uses the official Node.js Release schedule, bundled for offline inspection; **Check updates** refreshes it from `nodejs/Release` on demand.
- **NVM default indicator:** displays both the resolved Node version and the configured alias (for example, `lts/*`). The Active Node label describes the version selected in a short-lived zSoft Manager NVM shell; a separate already-open terminal may differ.
- **Installed versions:** lists the version directories in `$NVM_DIR/versions/node`, sorted newest first, with a clear EOL warning for unsupported release lines. **Set default** changes the NVM default alias for **new shells only**. It does not remove versions, modify running shells, or overwrite system Node.
- **Global npm packages:** installed global packages for the default NVM-managed Node are listed locally. **Check updates** queries npm for outdated global packages. The **Update** action changes only one explicitly selected package; the `npm` package itself must be updated using the dedicated npm compatibility action above. Package scripts may execute as part of normal npm installs, so update only packages you trust.
- **No background checking or cleanup:** all update checks are manual; there is no background daemon, sudo, or automatic uninstall of old Node versions.

**Node.js release schedule:** https://github.com/nodejs/Release/blob/main/schedule.json  
**NVM documentation:** https://github.com/nvm-sh/nvm#usage  
**npm global updates:** https://docs.npmjs.com/cli/update.html

## AppImage installation and updates

Download the latest `zSoft-Manager-vX.Y.Z-x86_64.AppImage` from [GitHub Releases](https://github.com/azislamayuda/zSoft-Manager/releases) once a release exists, then:

```bash
mkdir -p "$HOME/Applications/zSoft-Manager"
install -m755 ~/Downloads/zSoft-Manager-v0.4.2-x86_64.AppImage   "$HOME/Applications/zSoft-Manager/zSoft-Manager.AppImage"
./scripts/install-desktop.sh
```

The example filename above refers to v0.4.2; adjust to the release you downloaded. The stable destination name is `zSoft-Manager.AppImage`. The application can update itself only from this managed subdirectory (not from `cargo run`, nor directly from the `~/Applications` root). The first click on the Update icon checks, and the second click confirms installation; restart after a successful self-update.

## Build an AppImage

```bash
cargo test
cargo build --release
bash scripts/build-appimage.sh
```

On x86_64 Ubuntu, the script downloads an official linuxdeploy binary if not already cached, assembles GTK4-linked components into an AppDir and writes the AppImage + SHA256 file to `dist/`. Runtime portability must still be smoke-tested on the target Ubuntu versions. It is also possible to create a release via the GitHub Actions workflow by pushing a tag, after building and testing locally.

Use GitHub Desktop for **Commit to main** and **Push origin**. After tests succeed, create the tag `v0.4.7` from GitHub's new-release page, rather than using Git CLI.

The GitHub Actions `release-appimage.yml` job will test, build, package, and upload the AppImage to GitHub Releases. A Release is required before the self-updater works; an empty repository is not a Release.

## GitHub Desktop workflow

The development repository is expected at `/home/lamayuda/ZDATA/ME/GITHUB/zSoft-Manager`. Open it in GitHub Desktop, review the changed files, click **Commit to main**, then **Push origin**. Do not copy or overwrite `.git` from a source bundle. `target/`, `build/`, and `dist/` are generated locally and excluded from Git.

## Previous zAppImage Manager migration

If you already cloned `zSoft-Manager` via GitHub Desktop, **do not run the old migration script again**. Extract the v0.4.7 PATCH into your existing clone. Do not overwrite `.git`. Local Git operations are performed with GitHub Desktop, not the command line.

Source configuration reads existing `~/.config/zappimage-manager/sources.json` and state `~/.local/state/zappimage-manager/installed.json` as a fallback, and saves future changes under `zsoft-manager`. Old files remain untouched and existing rollback backups are preserved.

## UI status messages

Examples: `Changes saved successfully`, `Scan complete`, `Update finished`, `Rollback finished`, `Self-update failed`. Toast notifications disappear after 5 seconds. Check details in the per-app status and footer.

## License

Copyright (c) 2026 Azis Lamayuda. **Personal and non-commercial use only. No sale or commercial redistribution.** Source is publicly viewable and modifiable on those terms. See [LICENSE](LICENSE).

**Important:** this is a **source-available license**, not an OSI-approved open-source license. Preexisting releases of zAppImage Manager originally licensed MIT remain covered by their original license; existing rights are not revoked.

## Project layout

- `src/ui.rs` — GTK4 screens, navigation, toast notifications, actions
- `src/python_env.rs` — Python venv and pip inspection/update logic
- `src/python_ui.rs` — Python GTK4 page, activation, package selection
- `src/config.rs` — sources / version state with legacy fallback
- `src/update.rs` — GitHub + KDE downloads, validation, rollback
- `src/self_update.rs` — own GitHub Release check + guarded update
- `src/node.rs`, `src/node_ui.rs` — native Node toolchain backend and flat GTK4 page
- `src/naming.rs`, `src/scan.rs` — stable filename and scan rules
- `scripts/build-appimage.sh` — AppImage builder
- `.github/workflows/release-appimage.yml` — CI build / release

Please use the issue tracker for feedback and bug reports.


## v0.4.3 self-update readiness and performance

Self-update checks only offer installation after a matching published AppImage and a valid SHA256 digest (GitHub API digest or `.AppImage.sha256` release asset) are available. If GitHub has published the release before uploading its files, the in-app header icon becomes **Retry**; click it again after upload completes. The app does not poll in the background.

After self-installation, the application verifies the final installed file, and attempts to roll back if verification fails. Restart zSoft Manager after a successful update.

The Node.js and Python management pages are constructed only when first opened. AppImage scans reuse their existing GTK rows when the file list is unchanged. The Node view avoids deep-cloning the release schedule and global package inventory on each repaint. This is intended to reduce initial allocations and unnecessary work; actual memory usage depends on GTK4, distribution and workload.


## v0.4.4 AppImage compatibility and safe release fallback

The GitHub AppImage updater accepts generic architecture filename aliases (`x86_64`, `x64`, `amd64`) and AppImages without architecture labels. Explicit product-specific patterns are never broadened automatically. It excludes foreign architectures by name and validates the downloaded executable's 64-bit x86_64 ELF machine type before installation.

If the latest stable GitHub release is mobile-only, has no AppImage, or offers only AppImages for other architectures, the checker inspects up to 12 recent stable releases for the latest suitable AppImage. An unfinished upload or SHA256 checksum does **not** trigger fallback. In-app self-update always requires the latest release and verified SHA256, and never falls back.

Diagnostic errors distinguish **Network error**, **GitHub API HTTP error**, **Asset pattern mismatch**, **Architecture mismatch**, **Ambiguous AppImage assets**, and **No compatible x86_64 AppImage**.

A network-free regression suite exercises seven publisher scenarios from `tests/fixtures/github-appimage-regressions.json`. Verified asset filenames are identified in fixture provenance; representative names must be checked via `bash scripts/inspect-github-appimages.sh` before assuming upstream uses that exact string. This optional live audit only lists GitHub Release assets and does not download or install AppImages.

Before packaging: `bash scripts/audit-v044.sh`, then `cargo run --release`. Test AppImage Check updates with existing configured source patterns and confirm intended asset/version before clicking Update. The full AppImage build follows only after local tests pass.

### Matching manually installed versionless AppImages (v0.4.4)

On-demand GitHub update checks compare publisher SHA256 digests **and** published `.AppImage.sha256` sidecars to the bytes in a local renamed AppImage. When they match, status is **Up to date**, with no reinstall. If a checksum is unavailable or does not match, zSoft Manager will not guess: use **Set local version** after checking the version in the installed application's About window. Such an assertion is recorded against the exact local file SHA256 and repository; a replaced AppImage or changed repository invalidates it. Its status is marked **User-confirmed** rather than publisher-verified. Once current, the row displays a disabled **Up to date** check action; **Edit local version** is available from **Source** if a user-confirmation was saved and the release has been checked. If an older version is recorded, **Install update** remains available. No automatic background checks, reinstalls or downloads occur.

## Unified check-update notifications (v0.4.6)

- On every explicit **Check updates**, AppImage, Flatpak, Node.js and Python pages show a bottom-centred toast with their update counts. The shared toast uses sequence-safe auto-dismiss so a previous timeout cannot hide a new message.
- Flatpak summaries separately count application and runtime updates, current refs, and remote check errors, regardless of the currently selected tab. These counts come from Flatpak ref update status, **not** version-string guesses.
- Node.js summaries count available NVM/Node LTS/Current/npm updates and global npm packages separately, reporting warnings for partial checks.
- Python summaries show top-level outdated packages and installed top-level package count; an inactive Python environment is not treated as checked.
- AppImage summaries include confirmed updates, up-to-date items, unverified local versions, and source errors. Empty results still produce a toast.
- **Flatpak Applications / Runtimes** tabs now sit on the same row as the Flatpak Manager heading, aligned right.

## Flatpak Manager (v0.4.5)

The **Flatpak** header icon is located immediately after AppImage. It opens a dedicated page with separate **Applications** and **Runtimes** views. Flatpak data is loaded only when this page is opened.

- **Rescan** reads local user/system Flatpak installations only. No network access.
- **Check updates** queries configured Flatpak remotes for installed/available versions, commits, branches and architectures. Remote update status is authoritative even if version strings are equal or missing.
- **Select all / Clear** control which available refs are selected. **Update selected** only updates chosen refs; **Update all** applies to all available refs in the visible tab (Applications or Runtimes).
- The UI shows name, application ID, scope, origin, installed version, latest remote version (if advertised), and update status. A missing remote version is shown as unknown, never fabricated.
- User and system installations are explicitly separated. System updates may require an authorization dialog via Flatpak/Polkit. **Never launch zSoft Manager as root.**
- All Flatpak operations use argv-only subprocess calls, in worker threads, with no shell interpolation or daemon. The GUI reports progress per ref and errors without hanging.
- Requires the `flatpak` CLI installed and configured remotes (e.g. Flathub). If missing, this page shows an error without disrupting AppImage/Node/Python.
- v0.4.5 handles the standard `--user` and `--system` installations. Named additional system installations are not yet managed by this page.

For manual diagnostics: `flatpak list --app`, `flatpak list --runtime`, and `flatpak remote-ls --updates`.

## Ubuntu APT Manager (v0.4.7)

APT Manager appears immediately after Flatpak in the top navigation. Use Check updates for a read-only upgrade/autoremove simulation; Refresh lists requests Polkit authorization to run apt-get update. Upgrade packages and Autoremove require preview and confirmation before a second simulation and authorized operation. Autoclean also requires confirmation. The UI does not run as root. Protected system package removal, implicit full-upgrade, forced lock removal, and transactions above 25 changed packages are blocked in this first version. See `APT-v0.4.7-TEST-NOTES.md` for details.


## Dashboard and combined About page (v0.4.7)

Dashboard is now the first page and first navigation icon. Its five cards summarize **installed local software** separately: managed AppImage files, Flatpak applications (with runtime count), installed Ubuntu APT packages, NVM Node.js versions and global npm packages, and selected Python venv top-level packages. Each card shows only its **count and a concise summary**, without per-card buttons or individual application names. The layout uses three compact columns in a normal window and four columns when maximized, with rows retaining their natural height. **Refresh overview** reruns read-only local inventory workers. No GitHub/Flathub/APT repository updates, upgrades, or administrative prompts are triggered by Dashboard.

APT package totals are system packages, **not the number of graphical applications**. Unavailable CLI or invalid Python environments are shown as unavailable, never silently reported as zero. Dashboard counts are snapshots until refreshed; checks for available updates still belong in the individual manager pages.

About and Changelog share one navigation page, with separate scrollable cards. The obsolete Back to applications button and Network detail row were removed. See `docs/development/DASHBOARD-REFINE-v0.4.7-TEST-NOTES.md` for the current GUI verification steps.
