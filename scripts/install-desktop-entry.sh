#!/usr/bin/env bash
# Installs Cardboard's icon and a .desktop entry for the current user, so launchers and
# Wayland compositors (which match the window's app id "cardboard") show the icon.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
install -Dm644 "$root/assets/cardboard.svg" "$data/icons/hicolor/scalable/apps/cardboard.svg"
for size in 16 32 48 64 128 256 512; do
  if command -v rsvg-convert >/dev/null; then
    mkdir -p "$data/icons/hicolor/${size}x${size}/apps"
    rsvg-convert -w "$size" -h "$size" "$root/assets/cardboard.svg" -o "$data/icons/hicolor/${size}x${size}/apps/cardboard.png"
  fi
done
install -Dm644 /dev/stdin "$data/applications/cardboard.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Cardboard
Comment=Local-first HTML/CSS design editor
Exec=$root/scripts/run-desktop.sh
Icon=cardboard
Terminal=false
Categories=Graphics;Development;
StartupWMClass=cardboard
DESKTOP
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$data/icons/hicolor" || true
command -v update-desktop-database >/dev/null && update-desktop-database -q "$data/applications" || true
echo "Installed $data/applications/cardboard.desktop"
