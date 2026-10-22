# zSoft Manager v0.4.7 — Flatpak rows and APT icon

## What's changed

- Flatpak Apps and Runtimes rows now use visually aligned, divider-free columns within the existing separate cards.
- Column 1: app name and full Application ID (tooltip: scope, origin, branch).
- Column 2: architecture (e.g. `Arch: x86_64`).
- Column 3: installed and latest version; meaningful status after an update check.
- Per-item checkbox remains and only enables for confirmed updates.
- The repetitive text `Click Check updates` no longer appears in each unchecked row.
- APT uses the software package symbolic icon in both the main navigation and Dashboard, distinct from Flatpak's add-on icon.

## Testing on Ubuntu

1. From project root: `cargo fmt && bash scripts/audit-v047.sh`.
2. Run `cargo run --release`.
3. Flatpak > Applications: verify 10 app cards, app name/ID, Arch, Installed and Latest aligned in columns, with no repeated placeholder text.
4. Switch to Runtimes: verify each distinct ref and branch remains a separate card.
5. Click Check updates: statuses should appear and Update selected / Update all should retain previous safety rules.
6. Verify Flatpak and APT navigation icons differ and APT Dashboard card uses the software package icon.
7. Check normal and maximized window widths for clipped long IDs; use tooltips to view any truncated text.

No automatic Flatpak update, package removal, or privileged APT action is performed by these changes.
