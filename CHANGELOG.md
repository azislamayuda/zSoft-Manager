# Changelog

All notable changes to this project are recorded here. Format follows Keep a Changelog.

## [0.4.5] - 2026-10-21 (Development)

### Node.js cleanup (pre-release improvement)
- Add **Remove old** next to a Node version only when a newer version of the same major line is installed (e.g. keep v26.12.1 while optionally removing v26.12.0).
- For Current updates within the same major, migrate global npm packages with `--reinstall-packages-from` before cleanup, matching the previous `znode.sh` workflow.
- Ask for explicit confirmation before calling `nvm uninstall`; explain that version-specific global npm packages are deleted.
- Recheck NVM default/active version and availability of a newer same-major version immediately before uninstalling. Do not remove unrelated major lines, change the saved default alias, or touch system Node.js.
- Refresh the Node.js list after a successful removal. Add offline regression tests for cleanup candidate safety.
- Correct the v0.4.5 About version assertion from the earlier development patch.

### Flatpak UI compile fix
- Fixed invalid `ScanMessage::Finished` enum declaration in the Flatpak UI that prevented Rust formatting and compilation.

### Added

- New Flatpak icon immediately after AppImage in the top navigation, with a dedicated GTK4 Flatpak Manager page. The page is lazy-initialized only when opened.
- Separate Applications and Runtimes views for Flatpak user/system installations.
- Installed app/runtime discovery, installed versions and remote versions (when available), branch, origin, and per-ref update status.
- On-demand check of configured Flatpak remotes, selective updates, Update all, and per-ref progress/error feedback.
- Explicit separation of user/system Flatpak update operations and Flatpak/Polkit authorization without elevated GUI privileges.
- Offline Rust regression tests for local/remote output parsing, correct ref/architecture matching, equal-version updates, scope isolation, missing remotes and malformed output.

### Safety and compatibility

- Updates are determined by Flatpak's ref update availability, **not solely by version strings**.
- Failed remote checks are never mislabeled Up to date. Commands do not invoke shell or sudo and are run off the GTK main thread.
- No background polling or Flatpak subprocess at application startup. Existing AppImage, Node.js, Python, checksum/rollback and self-update functionality preserved.

### Testing

- Ubuntu `cargo test`, `cargo check --release` and live GUI checks required before final AppImage release.

## [0.4.4] - 2026-10-21

### Added

- Smart x86_64 AppImage asset detection for generic patterns, accepting `x86_64`, `x64`, `amd64`, or unlabelled `.AppImage` filenames.
- Bounded fallback through 12 recent stable GitHub releases when the latest stable release contains no AppImage for this architecture (e.g., Obsidian mobile-only release).
- Distinct error messages for architecture mismatch, asset pattern mismatch, multiple candidate AppImages, missing AppImages, and GitHub network/API failures.
- Offline regression fixtures covering PDFCraft, Trilium, Obsidian, OmniGet, GitHub Copilot, Linux GitHub Desktop, and OpenPLC Editor; unverified illustrative filenames are labelled as such.
- Optional `scripts/inspect-github-appimages.sh` live GitHub API diagnostic, which does not install/update applications.

### Improved local version detection (pre-release fix)

- Compare a manually installed or renamed local AppImage against the selected GitHub release's SHA256 digest during an explicit Check updates request. Matching bytes are identified as up to date, even without a version in the filename or installed.json.
- Check the expected file size before hashing, and run `sha256sum` without executing the AppImage or expanding a shell command.
- Distinguish an unverified installed version from a confirmed newer release; offer manual reinstallation as an optional action inside version confirmation, not as a confirmed update.
- Added offline unit tests for matching and mismatching local checksums, different PDFCraft tag/asset version schemes, and missing checksums.

### Improved installed-version matching (pre-release follow-up)

- Read publisher `.AppImage.sha256` sidecar files during ordinary on-demand GitHub checks, not only self-updates, so manually downloaded files with unchanged bytes can be detected as **Up to date** without reinstalling.
- For versionless AppImages whose bytes cannot be verified against upstream, provide **Set local version** (later **Edit local version**) with an explicit user-confirmation dialog. The confirmation is tied to the local file SHA256, GitHub repository, and release tag; it expires automatically when the file changes or the source changes.
- Distinguish checksum-verified **Up to date** from **User-confirmed local version**. Do not automatically mark mismatching/missing checksums as current; no automatic re-download, AppImage execution or reinstallation.
- Keep the normal Update all behavior restricted to confirmed newer versions. Add regression tests for confirmed/current/older/changed files and `.sha256` parsing.

### UI polish — installed version already current (pre-release follow-up)

- Show a disabled **Up to date** action with a check icon for both SHA256-verified and user-confirmed current AppImages; no install or reinstall is offered in the row.
- Move **Edit local version** into the existing **Source** dialog for AppImages with a previously saved manual confirmation and a checked GitHub release. Editing does not download or modify the AppImage.
- Hide **Reinstall latest** inside the local-version editor when a matching version is already recognized. Keep manual confirmation explicitly labelled as not publisher-verified.
- Add unit tests distinguishing Up to date, Update available, local version unknown, and no-check states.

### Fixed and hardened

- Prevent falling back to stale releases when the newest AppImage is still uploading or its SHA256 checksum is pending.
- Reject untrusted ARM64/32-bit ELF AppImage content at download verification, even when the filename is unlabelled or mislabelled.
- Retain strict user-defined product patterns; ambiguous filenames require explicit selection rather than guessing.
- Preserve pre-existing SHA256 validation, checked download size, backup, rollback and strict self-update readiness from v0.4.3.

### Testing

- Added offline selection/status/architecture regression tests and retained existing backup, rollback and checksum tests. Rust/GTK4 build and GUI tests must be completed on Ubuntu before publishing.

## [0.4.3] - 2026-10-21

### Added

- GitHub self-update readiness states to distinguish an incomplete release upload from an incorrectly configured asset pattern.
- Manual Retry state on the existing self-update header icon; no background retry loop or daemon.
- SHA256 readiness check: self-updates require a valid GitHub asset digest or a published, verified `.sha256` sidecar before installation is offered.
- Post-install verification of installed AppImage size, ELF header and SHA256; if verification fails, attempt automatic rollback and report any restoration error.
- Offline tests for incomplete releases, checksum parsing, mismatched filenames and post-install verification.

### Optimized

- Delay allocation of Node.js and Python GTK pages until first navigation; Node.js no longer spawns a local probe on AppImage-only startup.
- Skip rebuilding the AppImage list when a rescan finds identical paths, keeping existing GTK widgets and callbacks.
- Avoid cloning the full Node.js release schedule and package inventory on every Node page refresh; borrow the snapshot and clone only the requested install version.
- Bound GitHub API responses to 2 MiB and checksum downloads to 4 KiB to avoid unlimited response buffering.
- Select AppImage release assets with borrowed iterators instead of allocating an additional candidate list.
- Preserve sequential, manual-only updates. No new background services or runtime dependencies.

### Fixed (pre-release audit)

- Fixed Rust E0505 borrow/move error in the GitHub asset selector by cloning only the release tag when constructing the result.
- Corrected the Bash `npm outdated` fallback which previously appended an extra closing brace to JSON, including when no packages were outdated.
- Prevented a GTK main-thread `RefCell` borrow panic after a failed Python environment or pip check.
- Reduced unnecessary Node.js version/global-package widget recreation while the UI enters or exits a busy state without a new inventory snapshot.
- Added offline regression tests for npm JSON output and empty/malformed pip responses.

### Notes

- This release does not guarantee a numerical RAM reduction on every system. Compare idle RSS and startup times before/after on the same Ubuntu environment.

## [0.4.2.1] - 2026-10-21

### Fixed

- Standardized all six header navigation icons (AppImage, Node.js, Python, About, Changelog, and self-update) with GTK symbolic icons at 20 px in consistent 44 px hit targets.
- Removed raster PNG-based Node.js and Python header icons, which previously rendered at inconsistent natural sizes on some GTK themes.
- Moved in-window toast notifications to the bottom-center and switched the entrance animation to slide upward; toasts remain above the fixed footer without creating extra windows.

### Changed

- Increased default application window height from 720 px to 780 px for more comfortable Node.js and Python pages.
- Kept flat transparent backgrounds for inactive header navigation icons and soft-blue highlighting only for the active page.
- Preserved all v0.4.2 application-management behavior; no changes to update or install operations.
- Represented the four-part public version `0.4.2.1` as valid Cargo SemVer `0.4.2+1`, mapped back to the public version for the About page, self-update comparisons, AppImage filenames, and GitHub tag validation.

## [0.4.2] - 2026-10-21

### Added

- Node.js lifecycle badges on Active Node and installed versions: LTS (Active/Maintenance), Current, EOL, Unknown. Uses bundled official Node.js release schedule offline; Check updates refreshes the schedule from nodejs/Release on demand.
- Active-in-app and NVM Default Node indicators, including raw default alias and resolved version.
- Installed NVM Node versions, ordered numerically, with per-version **Set default** actions for future NVM shells.
- Global npm package inventory for the selected default NVM Node, checked locally on page load. Online outdated checks and per-package update buttons require manual action.
- Rust tests for lifecycle boundaries, default parsing, version enumeration, safe npm package names and JSON parsing.

### Changed

- v0.4.2 Node.js page contains sectioned flat cards and status badges matching the soft-blue theme.
- Optional Node Current installation preserves the previously selected NVM default alias.
- README now documents Node version selection and npm global package management.

### Safety

- NVM switching changes the default alias for **new** NVM shells, not already-open terminals.
- Global npm updates target a single validated package in the NVM environment; npm itself is managed using the dedicated compatible npm updater.
- Existing AppImage and Python features are unchanged, with no automatic cleanup, sudo, or idle network requests.

## [0.4.1] - 2026-10-21

### Added

- Python Environment page beside Node.js with a configurable `~/myenv` (or absolute-path) virtual environment.
- Session-scoped Activate/Deactivate button; only zSoft Manager subprocesses use the environment, not external terminals.
- Local Python, pip and top-level package inventory; on-demand `pip list --outdated --not-required --format=json`.
- Selectable outdated top-level packages and explicit Update selected; uses the venv's interpreter and runs `pip check` afterward.
- JSON preferences for the selected virtualenv path at `~/.config/zsoft-manager/python.json`.
- Offline Rust tests for path validation, pip JSON parsing, package-name validation and missing environment safety.
- Example GitHub Copilot Desktop AppImage mapping (`github/app`, Linux x86_64 asset) in examples.

### Changed

- Added Python page icon to the flat, active-only-highlight navigation.
- Updated AppImage testing script to read the release version from Cargo.toml instead of hard-coding 0.4.0.
- Updated help documents for Bootstrap Studio's license-aware built-in updater and distinct GitHub Copilot distributions.

### Safety

- No automatic package upgrades, no system-wide pip changes, no sudo, no remote scripts or idle network polling.
- Upgrades are limited to checked and selected top-level outdated packages; older environments remain untouched.
- A failed `pip check` is reported as a dependency warning rather than false success.
- Version v0.4.1 is source code ready for local Rust/GTK4 compilation; portable AppImage must be built and tested on Ubuntu.

## [0.4.0] - 2026-10-21

### Added

- A dedicated **Node.js** page with native GTK4 controls for installed NVM, Node LTS, npm and optional Node Current.
- Separate **Refresh installed**, **Check updates**, and per-tool installation actions; the page runs no remote requests during startup.
- NVM integration using a short-lived Bash subprocess to load `nvm.sh` safely; remote checks query NVM, Node.js and npm only on demand.
- Rust version parsing tests, Git tag selection tests, current-major detection tests and local output parsing tests.
- Scalable Zs SVG and matching PNG icon installed by the AppImage and desktop launcher scripts.

### Changed

- The header now shows **AppImage**, **Node.js**, **About**, **Changelog** and the existing self-update action.
- Page-navigation icon buttons are transparent unless selected, when soft-blue highlighting is applied.
- GTK4 `HeaderBar` is used to group minimize/maximize/close buttons more tightly; exact spacing can vary by desktop theme.
- About text, README, desktop metadata and package version updated for Node.js functionality.
- Packaging continues to produce `zSoft-Manager-v0.4.0-x86_64.AppImage` plus SHA256 once built on a supported Ubuntu system.

### Safety

- Existing NVM installations are managed without sudo, remote installer execution, automatic version deletion, or background polling.
- Node LTS remains the default even after optional Node Current installation.
- System Node.js installations outside NVM are detected but not overwritten.
- Build and runtime testing must be performed on Ubuntu; source archives are not precompiled AppImages.


## [0.3.0] - 2026-10-21

### Added

- Rebranded the application and Rust package as **zSoft Manager**, retaining current AppImage management functionality.
- In-window transient toast notifications for Source Save, scanning, rename, rollback, update, error states, and self-update.
- Header Update icon next to Changelog: manual GitHub Release check followed by explicit installation on the second click for supported AppImage installations.
- New AppImage packaging script, desktop metadata, icon, and tag-triggered GitHub Actions release workflow.
- Safe migration script importing original local Git history into the already-cloned GitHub worktree.
- README for GitHub and personal-use, non-commercial source-available LICENSE.

### Changed

- New application ID, executable name, desktop entry, and version: `zsoft-manager` 0.3.0.
- Config and state write to `zsoft-manager` XDG directories; legacy zAppImage Manager data remains readable for migration.
- The old rollback backup suffix stays compatible with previously saved backup files.

### Limitations

- AppImage binary must be built on Ubuntu or a GitHub Actions runner; this source bundle itself is not a compiled AppImage.
- Self-update requires a published GitHub Release and installation inside a `~/Applications` subfolder.
- The new noncommercial source-available license does not retroactively remove prior MIT grants.



## [0.2.4] - 2026-10-21

### Added

- Official KDE Kdenlive stable AppImage source, with automatic discovery for Kdenlive executables inside managed subfolders.
- Dynamic KDE release-series discovery (not hardcoded to 26.08) and latest x86_64 stable asset selection.
- KDE provider selector in the existing in-window Source sheet; no GitHub repository necessary for Kdenlive.
- Strict HTTPS-only KDE listing and asset download paths, exact remote size requirement, local ELF validation, and fallback errors if a mirror does not publish file length.
- Unit tests for KDE HTML discovery and filtering, asset version ordering, HTTP header length parsing, built-in source selection, and JSON compatibility.

### Changed

- Per-app action order is **Source, Stable name, Install update, Rollback**; Update and Rollback positions are swapped.
- Version bumped to 0.2.4. All interface labels remain in English; stable filename, backup, and rollback behaviors are unchanged.

### Security notes

- Only official stable `kdenlive-<version>-x86_64.AppImage` names are accepted from KDE directory listings. Mirrors must be HTTPS.
- No GPG signature verification is implemented. A missing KDE asset checksum is not treated as a verified SHA256.
- If KDE HEAD lacks a reliable `Content-Length`, the update is not offered rather than bypassing size verification.

## [0.2.3] - 2026-10-21

### Changed

- Replaced the GitHub Source popup with a native in-window GTK4 overlay sheet. It no longer creates a separate window or a second Ubuntu Dock indicator.
- Applied the same in-window treatment to the Stable name rename confirmation.
- Redesigned **Cancel**, **Save**, and **Rename** actions with left-hand icons, modern flat soft-blue styling, consistent button sizes, and generous padding on every edge.
- Added a close icon, keyboard Escape dismissal, comfortable form field spacing, and inline validation messages.
- Reused the existing GTK4 dependency; no extra runtime or background service.

### Tests

- Added source validation tests for valid mappings, invalid repository names, unsafe stable names, and installed filename mismatch.

### Preserved

- Update source JSON format, scanner exclusions, stable filenames, updates, backup, and rollback.

## [0.2.2] - 2026-10-21

### Added

- Three in-window pages using `GtkStack`: Applications, About, and Changelog, with a lightweight crossfade transition.
- Two compact, adjacent icon-only header navigation buttons with tooltips, active-page states, and in-page navigation back to Applications.
- Meaningful icons before labels on the three main actions (Rescan, Check updates, Update all) and the per-app action buttons.
- Flat, modern soft-blue styling with restrained borders, generous spacing, and accessible text labels for main actions.
- Unit tests for the page identifiers and English labels on main actions.

### Changed

- Translated all visible GUI controls, tooltips, progress messages, confirmation dialogs, and error notices into English.
- About and Changelog are now full pages inside the existing main window, rather than separate popup dialogs/windows.
- Changelog history and README now use English UI terminology.
- Footer GitHub link and automatic version display remain visible across all three pages.
- Updated Cargo version to `0.2.2` without adding dependencies, a web engine, or idle-time network requests.

### Preserved

- Existing scanning exclusions, GitHub release checks, manual updates, stable naming, backup, and rollback behavior.

## [0.2.1] - 2026-10-21

### Added

- Compact footer on the bottom right with the version read from `Cargo.toml` and an embedded GitHub icon linking to `https://github.com/azislamayuda`.
- Header **Menu** with **About** (native GTK4 About dialog) and **Changelog** (scrollable changelog window).
- Changelog content and GitHub icon are embedded in the binary at compile time; both features work offline without runtime asset paths or additional dependencies.
- Tests for version metadata, verified GitHub profile constant, embedded changelog, and icon asset.

### Changed

- Bumped version to `0.2.1`. Existing AppImage scanning, stable naming, updates, and rollback are unchanged.

## [0.2.0] - 2026-10-21

### Added

- **Stable versionless AppImage names**: a downloaded release replaces `<AppName>.AppImage`, not a version-bearing filename.
- **Stable name** button: manually migrate an installed AppImage to a versionless name without downloading it again, with a confirmation dialog.
- Editable stable filename in the **Source** dialog; source mappings remain valid after a versioned-to-stable migration.
- Relative compatibility symlink at the previous filename so older launchers continue to work; the scanner deliberately ignores symlinks.
- Best-effort updates of user-owned `Exec=` and `TryExec=` desktop entries in `~/.local/share/applications`.
- Previous installed version tracking (`previous_tags`) so rollback maintains accurate comparison when known.
- Unit tests for filename inference, invalid names, existing-file collision, rename without byte changes, versionless install and rollback, backup rotation, and backward-compatible configuration.

### Changed

- Backups for migrated apps are now located at `.<stable filename>.zappimage-manager.backup`.
- GUI rescans the folder after finished installations to show the new stable filename immediately.
- Version bumped to `0.2.0`; uses the existing size-optimized Rust/GTK4 build configuration.

### Safety

- Aborts migration when the stable target already exists, including a broken symlink.
- Prevents manual update/rename/rollback for files outside `~/Applications` subfolders or inside `ArtCraft`.
- Creates a backup before replacing any AppImage; restores the original when the initial rename/install fails.
- Never deletes an original file during failed download or checksum verification.
- Compatibility symlinks are not followed by the AppImage scanner.

### Limitations

- Rename inference is heuristic. Edit the desired filename in the **Stable name** confirmation dialog, and configure it in **Source** for subsequent updates.
- `.desktop` repair covers files immediately inside `~/.local/share/applications` only; other launchers rely on the legacy compatibility symlink.
- Only one prior AppImage update is kept for rollback; no guarantee against abrupt power loss during the two-step rename or filesystem corruption.
- GitHub Releases only; manual normalization works without a configured GitHub source.
- Build and tests require a system with Rust and GTK4 development headers.

## [0.1.0] - 2026-10-21

### Added

- Native Rust/GTK4 application with a soft-blue GUI.
- Recursive scan of `.AppImage` files only inside `~/Applications` subfolders, excluding `ArtCraft` and symlinks.
- Per-application GitHub release configuration, version checks, size and optional SHA256 verification.
- User-invoked downloads, updates, one-generation backups and rollback.
- Lightweight Cargo release profile, README, `.gitignore`, local Git and unit tests.

### Fixed

- Fix Rust compiler error `E0716` in `src/ui.rs` by storing the formatted GTK status text in an owned `String`.
