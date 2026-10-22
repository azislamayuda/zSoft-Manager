# APT Manager v0.4.7: test and safety notes

Status: DEVELOPMENT BUILD. Do not publish before tests on Ubuntu.

## Design

- Navigation icon after Flatpak; APT page is created lazily.
- `Check updates`: read-only, runs `/usr/bin/apt-get -s upgrade`, `/usr/bin/apt-get -s autoremove`, and `apt-mark showhold`. It does **not** refresh indexes or request authentication.
- `Refresh lists`: after explicit confirmation, runs `pkexec /usr/bin/apt-get ... update`. Authentication is handled by the OS Polkit agent; zSoft does not request or store a password.
- `Upgrade packages`: preview lists packages/versions; confirm, then simulate again. If changed, refuse. Use `pkexec apt-get --assume-yes --no-remove upgrade`. This does not offer `full-upgrade`.
- `Preview Autoremove`: explicit package removal list and confirmation; reject protected families (kernel, grub, Ubuntu meta-packages, systemd, libc, dpkg, apt, network-manager, GDM, firmware, Nvidia). Runs non-purging `autoremove` only.
- `Autoclean`: explicit confirmation, runs native `apt-get autoclean` via Polkit. Exact reclaimable space is unknown before execution.
- APT/dpkg locks are owned by APT. Never remove lock files or kill package managers.
- APT phased updates/held packages are not overridden; `apt-mark showhold` shows explicitly held packages; the APT simulation summary shows packages not upgraded.
- APT and dpkg errors are shown in a live, truncated **in-session** log. If the UI log truncates old lines, inspect `/var/log/apt/history.log` and `/var/log/apt/term.log` on the host as appropriate.
- Detects `/var/run/reboot-required`; it does not reboot automatically.
- Preview is checked immediately before authorization, but APT state may theoretically change after the check and before APT acquires its lock. This is **not** a transactional or immutable plan. APT's own safety checks remain essential.
- Transactions with more than 25 listed package changes are blocked in this first release because the confirmation dialog is intentionally limited.
- Package versions shown are inferred from `apt-get -s` output. Repository index freshness depends on `Refresh lists`.
- No custom privileged executable, unattended scheduling, interactive terminal root session, or permanent background daemon is installed. `/usr/bin/apt-get` is the allowlisted native privileged executable, invoked with programmatic fixed arguments (never a shell).
- APT's own maintainer scripts run with system privileges, as with every `apt-get` upgrade; this cannot be made risk-free by the GUI.

## Prerequisites

Ubuntu GTK4 and Rust build dependencies as in `BUILD-RELEASE.md` plus `pkexec` and a working graphical Polkit agent. To check: `command -v pkexec apt-get apt-mark`.

## Audit

```bash
cd "$HOME/ZDATA/ME/GITHUB/zSoft-Manager"
cargo fmt
bash scripts/audit-v047.sh
cargo run --release
```

## GUI test sequence (start read-only)

1. Click APT (icon directly after Flatpak). Verify it loads lazily, system packages are not changed.
2. Click Check updates; compare the upgrade list with `apt-get -s upgrade` and holds with `apt-mark showhold`.
3. Click Upgrade packages and **Cancel** the confirmation. No privilege prompt or package changes should occur.
4. Click Preview Autoremove and **Cancel**. Verify the list of removable packages; protected packages must cause a blocked status.
5. Click Autoclean and **Cancel**. No auth prompt should appear.
6. Click Refresh lists then **Cancel** to test Polkit cancellation, when available. No zSoft password prompt appears.
7. Only when you have a backup and the preview is reviewed: manually approve `Refresh lists`. Review the operation log and rescanned list.
8. Test genuine Upgrade/Autoremove/Autoclean selectively, when appropriate; confirm resulting APT history. Avoid testing destructive operations just to make tests pass.
9. If `unattended-upgrades` holds the lock, verify the operation stops with a readable error. Do not forcibly remove APT locks.
10. Verify AppImage, Flatpak, Node.js, Python, toast notifications, and window layout are unchanged.

## Known limitations

- No full-upgrade, purge, PPA management, distribution upgrade, arbitrary package installation/removal, or automatic cleanup.
- No package-specific selection for APT upgrade in v0.4.7; standard `apt-get upgrade` applies eligible safe upgrades after preview.
- Interactive maintainer scripts can still require extra attention; noninteractive invocation can fail. The application should not be used to bypass prompts; use a terminal/Ubuntu Software Updater when needed.
- The GUI log is in-session and may truncate old output. APT system logs are maintained by Ubuntu.
- APT package changes cannot be rolled back like AppImages; system backups/snapshots are recommended.
