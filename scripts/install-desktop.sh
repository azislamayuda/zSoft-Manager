#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
release_bin="$project_dir/target/release/zsoft-manager"
installed_image="$HOME/Applications/zSoft-Manager/zSoft-Manager.AppImage"
if [[ -f "$installed_image" ]]; then
  executable="$installed_image"
elif [[ -x "$release_bin" ]]; then
  executable="$release_bin"
else
  printf 'Build with cargo build --release or install the AppImage first.\n' >&2
  exit 1
fi
launcher="${XDG_DATA_HOME:-$HOME/.local/share}/applications/io.github.azislamayuda.zsoft-manager.desktop"
install -d "$(dirname "$launcher")"
icon_dir="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps"
install -Dm644 "$project_dir/assets/zsoft-manager.png" "$icon_dir/zsoft-manager.png"
install -Dm644 "$project_dir/assets/zsoft-manager.svg" "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps/zsoft-manager.svg"
cat > "$launcher" <<EOF
[Desktop Entry]
Type=Application
Name=zSoft Manager
Comment=Native Rust GTK4 manager for AppImage, Node.js, npm and NVM updates
Exec="$executable"
Icon=zsoft-manager
Terminal=false
Categories=Utility;System;
StartupWMClass=io.github.azislamayuda.zsoft-manager
StartupNotify=true
EOF
printf 'Installed desktop launcher: %s\n' "$launcher"
printf 'Note: older zAppImage Manager shortcuts are not removed automatically.\n'
