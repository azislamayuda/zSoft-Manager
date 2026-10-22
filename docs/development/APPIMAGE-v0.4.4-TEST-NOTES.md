# zSoft Manager v0.4.4 — GitHub AppImage detection tests

This change is for the v0.4.4 development source, **not** a v0.4.3 hotfix.

## Scope

1. Generic pattern aliases: x86_64 / x64 / amd64 / unlabelled AppImage. Case-insensitive .AppImage suffix.
2. No silent override of a custom product-specific pattern; patterns matching more than one AppImage are rejected.
3. Fallback checks up to 12 recent stable GitHub releases only if the latest stable release lacks x86_64 AppImages; skips drafts/prereleases and incomplete uploads.
4. Missing AppImage, wrong architecture, pattern mismatch, ambiguity, API/network errors and upload pending produce different messages.
5. Selected downloadable AppImages must be 64-bit little-endian x86_64 ELF (ELF machine 62), in addition to exact size and optional SHA256 validation. Old binary is backed up for rollback.
6. Self-update stays latest-only and checksum-required. No automatic background polling.

## Offline regression coverage: seven applications

- PDFCraftTool/pdfcraft: user-observed `PDFCraft_0.1.0_amd64.AppImage` historical name (newer release names may differ).
- TriliumNext/Trilium: verified `TriliumNotes-v0.106.0-linux-x64.AppImage` and ARM64 variant.
- obsidianmd/obsidian-releases: verified `Obsidian-1.13.7.AppImage` plus ARM64; mobile-only `1.13.8` fallback.
- OpenSelena/omniget: representative amd64 + arm64 names based on upstream's advertised architectures; confirm live actual asset strings.
- github/app: verified `GitHub-Copilot-linux-x64.AppImage` and ARM64.
- shiftkey/desktop: representative x64/ARM64 filename based on upstream GitHub Desktop Linux assets; confirm live actual asset strings.
- Autonomy-Logic/openplc-editor: representative x64/ARM64 filename based on upstream's Linux build support; confirm live actual asset strings.

The fixtures are **offline test cases**, not a complete inventory of current upstream releases. To print exact current GitHub names use:

```bash
bash scripts/inspect-github-appimages.sh
```

This makes GitHub read-only requests (may be rate limited) and does not install anything.

## Ubuntu build + GUI verification

```bash
cd "$HOME/ZDATA/ME/GITHUB/zSoft-Manager"
cargo fmt
bash scripts/audit-v044.sh
cargo run --release
```

Check each AppImage source. For app-specific patterns that do not match upstream, use a deliberate compatible pattern rather than a broad wildcard that matches multiple products. Check that ARM64 assets are never presented on x86_64. Confirm fallback on Obsidian's mobile-only latest release and correct error messages on offline/network failures.

Do not build the final AppImage until these checks pass. Build with `bash scripts/test-and-build.sh`, test the packaged AppImage, then commit with GitHub Desktop and publish v0.4.4.

## Limitations

- This artifact environment lacks the Rust/GTK4 toolchain; `cargo fmt`, `cargo test`, `cargo check`, and GUI tests have **not** run here.
- Upstream GitHub API names for every publisher were not all accessible during package preparation. Exact unverified strings must be confirmed with the optional diagnostic.
- Unlabelled AppImage filenames cannot establish architecture from the name; the downloaded ELF header is verified before installation.
- Generic fallback can select an older desktop release by design; always review target version and asset before applying an update.

## Local installed-version detection (pre-release fix)

If a local filename is versionless and the AppImage was copied manually, `installed.json` might lack its version. On user-initiated **Check updates**, zSoft Manager compares the local AppImage's size and SHA256 to the selected release if GitHub provides a SHA256 digest. Exact matches report **Up to date** without downloading or updating anything.

If no trustworthy SHA256 is available, or the bytes do not match, the version may remain **unverified**. This is not proof of an update. The row shows **Set local version** (no download), and this item is excluded from **Update all**. Manual reinstallation remains a separate optional action inside the confirmation dialog while the installed version is unverified. GitHub releases with different versions in tags and binary names (e.g. PDFCraft) are handled by checksum identity first.

AppImage checksums are calculated only during Check updates, not at startup or while idle. For large files, checking every installed AppImage may take longer and cause disk reads; no ongoing daemon is added. AppImage execution/extraction is not required.

### Ubuntu GUI smoke tests

1. Launch `cargo run --release`, open AppImage page, click Check updates. Confirm manually installed **Obsidian, OmniGet and PDFCraft** show **Up to date** when their bytes exactly match the selected GitHub asset.
2. For applications without a matching checksum, confirm **Local version unverified** and **Set local version** on the main row, not a confirmed update. The confirmation dialog may offer a separate optional **Reinstall latest** while unverified.
3. Confirm Update all excludes unverified entries.
4. Confirm normal versioned installs, source selection, self-update, rollback and backup behavior are unchanged.

The source files have been statically inspected but require Ubuntu cargo tests and GUI validation.

## Installed version matching (pre-release follow-up)

1. GitHub Desktop (`shiftkey/desktop`): Check updates; the checker now reads the `.AppImage.sha256` sidecar when available. Matching local SHA256 + file size should show `Up to date` automatically.
2. OpenPLC Editor (`Autonomy-Logic/openplc-editor`): if publisher SHA256 does not match an independently installed AppImage, the GUI must not claim it is the same binary. Use **Set local version** only after independently checking the installed app's version in its About dialog; the field is prefilled with the GitHub tag and can be edited.
3. After confirming the correct local version, Check updates again: `Up to date ... User-confirmed local version` without reinstalling. The main button becomes disabled **Up to date**; mistaken declarations can be corrected through **Source → Edit local version** without editing JSON. Record in `~/.local/state/zsoft-manager/installed.json` under `local_version_confirmations`.
4. Replace/change that local AppImage (without deleting the confirmation). Check again; the manual confirmation must no longer count as matching. Change source repository and check likewise.
5. If confirming an older installed tag (e.g. v4.3.1 vs latest v4.3.2), `Update available` should appear; the normal Update all can include it.
6. Do not edit checksum hashes manually. A user-confirmed version is a statement about version, **not** proof of file identity or publisher authenticity.
7. Source/install JSON backward compatibility: existing keys remain unchanged; no update/reinstall is performed by these checks.

## Final pre-release UI polish: already installed = Up to date

1. After OpenPLC Editor v4.3.2 is confirmed from About, click **Check updates**. The status must say **Up to date: v4.3.2** and **User-confirmed (not publisher-verified)**; the primary button must show a disabled **Up to date** with a check icon, not **Edit local version**, **Reinstall latest** or **Install update**.
2. Click **Source** for OpenPLC Editor. With a saved manual confirmation and a checked release, **Edit local version** appears next to Cancel and Save. Click it: the existing version field is editable; **Reinstall latest** must not be shown if the installed version is current.
3. Record a deliberately older version only if it matches the actual installed AppImage. Verify **Install update** is offered after Check updates, without requiring a reinstall when already current.
4. For a file whose publisher SHA256 matches, verify the status remains **Up to date** without needing manual confirmation. The local version editor should not be offered unless a prior user-confirmed record exists.
5. Changing the local AppImage invalidates a manual confirmation; after a new Check updates, a missing or mismatched publisher digest must not claim **Up to date**.
6. No change to GitHub asset selection, automatic downloads, checksum checking, rollback, Node.js or Python pages.
