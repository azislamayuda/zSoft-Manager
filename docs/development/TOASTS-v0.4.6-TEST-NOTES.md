# zSoft Manager v0.4.6 — Toast & Flatpak Layout Tests

## What changed

- Flatpak heading and Applications / Runtimes buttons now share one horizontal row; selector aligned right.
- AppImage, Flatpak, Node.js, Python show an accurate toast summary on explicit Check updates; local refresh remains quiet.
- AppImage reports available, up to date, unverified, source errors.
- Flatpak reports application updates, runtime updates, up to date refs, remote errors across **both** views.
- Node.js reports tool updates, global npm package updates, warnings.
- Python reports top-level outdated packages and installed package count.
- Shared toast auto-dismiss is sequence-gated and reused across pages.

## Ubuntu tests

1. `cargo fmt` then `bash scripts/audit-v046.sh`; fix any compile/test errors before GUI test.
2. `cargo run --release` and check footer `v.0.4.6`.
3. Open Flatpak; verify Applications / Runtimes buttons are aligned with the Flatpak Manager title on the right, with description below it.
4. With no Flatpak updates, click Check updates in either tab; expect `0 app update(s), 0 runtime update(s)` and a count of current refs, with `0 check error(s)` if all remotes succeeded. If a remote failed, show a nonzero error count instead.
5. Click Check updates in Node.js; ensure toast counts NVM/Node/npm and global packages separately, and reports warning count if any provider check failed.
6. Activate Python venv, then click Check updates; expect number of outdated top-level packages. If no venv is activated, update check remains disabled.
7. Click AppImage Check updates; expect counts for confirmed updates, current, unknown and source errors. No downloads or installations are performed by a check.
8. Trigger two checks on different pages within 5 seconds; the newer toast must not be dismissed prematurely by an older timer.
9. Test `bash scripts/test-and-build.sh` only after these checks pass, then verify packaged AppImage and checksum.

## Caveats

- No actual `flatpak update` is performed during a check. Updates continue to require an explicit separate click.
- Flatpak counts use **ref** update availability, not matching version strings.
- No online checks run at GUI startup beyond previous page behaviour.
- Rust toolchain and GTK4 dev dependencies are required to compile. This source has not been compiled in the packaging environment.
