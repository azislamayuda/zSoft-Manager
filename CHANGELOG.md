# Changelog

All notable changes to this project are recorded here. Format follows Keep a Changelog.

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
