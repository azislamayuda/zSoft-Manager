# Dashboard compact layout — zSoft Manager v0.4.7

This patch is intended for the **latest v0.4.7 Dashboard Refine** source. It changes Dashboard layout only; no package manager behavior or administrative privileges are modified.

## Changes

- Normal (restored) window: exactly **3 summary cards per row**.
- Maximized window: exactly **4 summary cards per row**.
- Cards keep a compact natural height instead of expanding vertically to fill the scroll viewport.
- Reduced card padding, minimum height and vertical internal spacing.
- Dashboard still shows **counts and short summaries only**; no buttons, application-name lists, remote checks or background polling.

## Install patch

```bash
cd "$HOME/ZDATA/ME/GITHUB/zSoft-Manager"
unzip -o ~/Downloads/zSoft-Manager-v0.4.7-DASHBOARD-COMPACT-PATCH.zip -d .
cargo fmt
bash scripts/audit-v047.sh
cargo run --release
```

## GTK GUI checklist

1. Start on Dashboard and maximize window: 4 columns (4 cards top, 1 card below), with no tall blank cards.
2. Restore window: 3 columns (3 cards top, 2 cards below).
3. Toggle maximize/restore repeatedly to ensure changes apply without refreshing.
4. Click Refresh overview to confirm read-only counts are updated and cards remain compact.
5. Verify the AppImage header, APT safety controls, Flatpak, Node.js, Python, and About/Changelog are unchanged.

No actual APT, Flatpak, AppImage or other package upgrade should be performed as part of this visual test.

**Validation:** The packaged code and file layout were inspected, and static checks were run. Cargo and GTK4 are not installed in the packaging environment, so compile/tests and pixel-level rendering must be verified on Ubuntu.
