# zSoft Manager v0.4.7 — APT header layout patch

This patch adjusts only the APT Manager page layout: **Check updates**, **Refresh lists**, and **Upgrade packages** appear right-aligned on the same line as **APT Manager**. The existing subtitle remains below the title row, and the Autoremove/Autoclean controls remain in their current position.

- No changes to APT command execution, authorization, locks, safety checks, previews, or callback handling.
- Remains version **0.4.7**.
- Includes one source regression test verifying the heading row structure.
- ZIP entries are relative to the repository root. No `.git` files are included.

## Install on Ubuntu (from VS Code terminal)

```bash
cd "$HOME/ZDATA/ME/GITHUB/zSoft-Manager"
unzip -o ~/Downloads/zSoft-Manager-v0.4.7-APT-HEADER-PATCH.zip -d .
cargo fmt
bash scripts/audit-v047.sh
cargo run --release
```

Check that the three primary action buttons are aligned to the right of the APT Manager heading and function as before. First test Check updates, then open the confirmation dialogs for privileged actions and choose Cancel. Do not perform real package changes for a layout-only test.

Note: This patch does not include or replace `scripts/audit-v047.sh`, so it preserves any local fix you have already made for its awk warning.
