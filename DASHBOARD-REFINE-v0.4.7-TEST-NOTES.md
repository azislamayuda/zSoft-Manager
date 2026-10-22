# Dashboard and AppImage UI refinements — zSoft Manager v0.4.7

Historical UI refinement. The compact-height follow-up is recorded in `DASHBOARD-COMPACT-v0.4.7-TEST-NOTES.md`.

This is a UI-only update on top of the **v0.4.7 Dashboard patch**. No application install/update behavior has been changed.

## Changes

- Dashboard cards have no action buttons and no application-name breakdown; each shows a count and concise summary.
- Normal window: **3 cards per row**. Maximized GTK window: **4 cards per row**. Maximizing/unmaximizing refreshes layout without background polling.
- AppImage page: **AppImage Manager** subtitle heading, action buttons right-aligned in the same row.
- Navigation: Flatpak uses the GTK theme add-on/puzzle symbol; APT uses the system/gear symbol. Their Dashboard card icons match.
- Dashboard performs local read-only inventory only; it never triggers GitHub, APT refresh, or package updates.

## Install PATCH

```bash
cd "$HOME/ZDATA/ME/GITHUB/zSoft-Manager"
unzip -o ~/Downloads/zSoft-Manager-v0.4.7-DASHBOARD-REFINE-PATCH.zip -d .
cargo fmt
bash scripts/audit-v047.sh
cargo run --release
```

## GUI checklist

1. Dashboard: confirm counts and summaries only; no per-card buttons or names.
2. Normal window: three columns; maximize: four; restore: three (verify layout updates immediately).
3. AppImage: verify its three actions are to the right of **AppImage Manager** and retain existing behavior.
4. Confirm Flatpak and APT navigation icons are visibly different on your Ubuntu theme.
5. Check pages APT, Flatpak, Node.js, Python and About/Changelog remain intact.
6. Do not perform privileged APT operations until the safeguards have been fully tested.

Static inspection was performed on the package, but Rust/GTK4 compilation requires the Ubuntu environment. Do not release AppImage until `cargo test`, GUI verification, and the existing APT safety checks pass.
