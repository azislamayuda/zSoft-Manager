# zSoft Manager v0.4.6 — GitHub API HTTP Diagnostic Fix

This patch applies to the v0.4.6 source after the unified update toast patch.

**Files:** `src/update.rs`, `src/ui.rs`, `CHANGELOG.md`.

The updater now reports GitHub HTTP status (403/404/429/5xx) and the short public GitHub error message. It does not change the AppImage asset-selection algorithm, SHA256 validation, backup, rollback or installation policy. When a GitHub rate limit is encountered, remaining GitHub checks within the same batch are skipped; KDE checks can still proceed.

Run on Ubuntu:

```bash
cargo fmt
bash scripts/audit-v046.sh
cargo run --release
```

For rate limit diagnostics without disclosing credentials:

```bash
curl -fsSL -H 'Accept: application/vnd.github+json' \
  -H 'User-Agent: zSoft-Manager/0.4.6' \
  https://api.github.com/rate_limit | python3 -c 'import sys,json,datetime; d=json.load(sys.stdin)["resources"]["core"];print("Remaining:",d["remaining"],"/",d["limit"]);print("Reset (local):",datetime.datetime.fromtimestamp(d["reset"]).astimezone().isoformat())'
```

A GitHub HTTP 403/429 error may be rate limiting, but the HTTP code/message must be inspected first. Do not reinstall an AppImage just because the check failed. GitHub recommends not retrying while rate limited.

This patch was statically reviewed; Rust compilation and GTK4 runtime need Ubuntu verification.
