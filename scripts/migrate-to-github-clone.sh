#!/usr/bin/env bash
set -euo pipefail
old="$HOME/ZDATA/MENTAHAN/zAppImage Manager"
new="$HOME/ZDATA/ME/GITHUB/zSoft-Manager"
[[ -d "$old/.git" ]] || { echo "Source .git missing: $old" >&2; exit 1; }
[[ -d "$new/.git" ]] || { echo "Destination clone .git missing: $new" >&2; exit 1; }
[[ -z "$(git -C "$old" status --porcelain)" ]] || { echo 'Commit or stash changes in the OLD project first.' >&2; exit 1; }
[[ -z "$(git -C "$new" status --porcelain)" ]] || { echo 'Commit or stash changes in the NEW clone first.' >&2; exit 1; }
# Import all historical commits to the existing clone without overwriting origin.
git -C "$new" fetch "$old" HEAD
git -C "$new" switch -C main FETCH_HEAD
printf '\nGit history migrated to: %s\n' "$new"
printf 'Next: extract zSoft-Manager-v0.3.0-PATCH.zip into the NEW directory.\n'
printf 'The old project is untouched and remains available as a backup.\n'
