#!/usr/bin/env bash
set -euo pipefail
# Packages an already compiled release binary using the official linuxdeploy AppImage.
# Requires network once to obtain linuxdeploy unless LINUXDEPLOY points to a cached copy.
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo"
[[ "$(uname -m)" == "x86_64" ]] || { echo 'Only x86_64 is supported.' >&2; exit 1; }
[[ -x target/release/zsoft-manager ]] || { echo 'Run cargo build --release first.' >&2; exit 1; }
version="$(sed -n '/^version *=/{s/.*= *"\([^"]*\)".*/\1/p;q;}' Cargo.toml)"
appdir="$repo/build/zSoftManager.AppDir"
mkdir -p "$repo/build/tools" "$repo/dist"
rm -rf "$appdir"
install -Dm755 target/release/zsoft-manager "$appdir/usr/bin/zsoft-manager"
install -Dm644 packaging/io.github.azislamayuda.zsoft-manager.desktop \
  "$appdir/usr/share/applications/io.github.azislamayuda.zsoft-manager.desktop"
install -Dm644 assets/zsoft-manager.png \
  "$appdir/usr/share/icons/hicolor/256x256/apps/zsoft-manager.png"
install -Dm644 assets/zsoft-manager.svg \
  "$appdir/usr/share/icons/hicolor/scalable/apps/zsoft-manager.svg"
# Prefer an explicitly provided tool for offline builds.
linuxdeploy="${LINUXDEPLOY:-$repo/build/tools/linuxdeploy-x86_64.AppImage}"
if [[ ! -f "$linuxdeploy" ]]; then
  curl --fail --location --show-error --silent --proto '=https' --proto-redir '=https' \
    --output "$linuxdeploy" \
    'https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage'
fi
chmod +x "$linuxdeploy"
# FUSE is often unavailable in CI; extraction fallback avoids needing /dev/fuse.
cd "$repo/build"
find "$repo/build" -maxdepth 1 -type f -name "*.AppImage" -delete
VERSION="$version" ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$linuxdeploy" \
  --appdir "$appdir" \
  --executable "$appdir/usr/bin/zsoft-manager" \
  --desktop-file "$appdir/usr/share/applications/io.github.azislamayuda.zsoft-manager.desktop" \
  --icon-file "$appdir/usr/share/icons/hicolor/256x256/apps/zsoft-manager.png" \
  --output appimage
mapfile -t images < <(find "$repo/build" -maxdepth 1 -type f -name '*.AppImage' -print)
if [[ "${#images[@]}" -ne 1 ]]; then
  printf 'Expected exactly one built AppImage, found %s.\n' "${#images[@]}" >&2
  exit 1
fi
output="$repo/dist/zSoft-Manager-v${version}-x86_64.AppImage"
install -Dm755 "${images[0]}" "$output"
(cd "$repo/dist" && sha256sum "$(basename "$output")" > "$(basename "$output").sha256")
printf 'AppImage built: %s\n' "$output"
