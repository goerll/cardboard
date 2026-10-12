#!/bin/sh
# Cardboard installer for Linux and macOS.
#   curl -fsSL https://cardboard.goerll.dev/install.sh | sh
# Re-run it to update. Set CARDBOARD_VERSION=v0.1.0 to pin a release.
set -eu

repo="goerll/cardboard"
version="${CARDBOARD_VERSION:-latest}"
bin_dir="${CARDBOARD_BIN_DIR:-$HOME/.local/bin}"

say() { printf '\033[1m%s\033[0m\n' "$*"; }
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "$1 is required"; }

case "$(uname -s)" in
  Linux) os=linux; ext=tar.gz ;;
  Darwin) os=macos; ext=zip ;;
  *) fail "unsupported system $(uname -s). On Windows run: irm https://cardboard.goerll.dev/install.ps1 | iex" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x64 ;;
  arm64 | aarch64) arch=arm64 ;;
  *) fail "unsupported architecture $(uname -m)" ;;
esac
[ "$os" = linux ] && [ "$arch" = arm64 ] && fail "no Linux arm64 build yet"

asset="cardboard-$os-$arch.$ext"
if [ "$version" = latest ]; then
  url="https://github.com/$repo/releases/latest/download/$asset"
else
  url="https://github.com/$repo/releases/download/$version/$asset"
fi

need curl
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM
say "Downloading $asset"
curl -fL --progress-bar "$url" -o "$tmp/$asset" || fail "download failed: $url"
mkdir -p "$bin_dir"

if [ "$os" = linux ]; then
  data="${XDG_DATA_HOME:-$HOME/.local/share}"
  dest="$data/cardboard/app"
  tar -xzf "$tmp/$asset" -C "$tmp"
  rm -rf "$dest" && mkdir -p "$(dirname "$dest")"
  mv "$tmp/cardboard-$os-$arch" "$dest"
  ln -sf "$dest/cardboard" "$bin_dir/cardboard"
  ln -sf "$dest/cardboard-desktop" "$bin_dir/cardboard-desktop"
  icons="$data/icons/hicolor"
  mkdir -p "$icons/scalable/apps" "$data/applications"
  cp "$dest/cardboard.svg" "$icons/scalable/apps/cardboard.svg"
  for size in 64 128 256 512; do
    mkdir -p "$icons/${size}x${size}/apps"
    cp "$dest/cardboard-$size.png" "$icons/${size}x${size}/apps/cardboard.png"
  done
  cat > "$data/applications/cardboard.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Cardboard
Comment=Local-first HTML/CSS design editor
Exec=$dest/cardboard-desktop
Icon=cardboard
Terminal=false
Categories=Graphics;Development;
StartupWMClass=cardboard
DESKTOP
  command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$icons" || true
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$data/applications" || true
  launch="Open Cardboard from your app launcher, or run: cardboard-desktop"
else
  need unzip
  apps=/Applications
  [ -w "$apps" ] || apps="$HOME/Applications"
  mkdir -p "$apps"
  unzip -q "$tmp/$asset" -d "$tmp/app"
  rm -rf "$apps/Cardboard.app"
  mv "$tmp/app/Cardboard.app" "$apps/Cardboard.app"
  # The app is not notarized by Apple; clear the download flag so Gatekeeper lets it open.
  xattr -dr com.apple.quarantine "$apps/Cardboard.app" 2>/dev/null || true
  ln -sf "$apps/Cardboard.app/Contents/MacOS/cardboard-cli" "$bin_dir/cardboard"
  launch="Open Cardboard from $apps, or run: open -a Cardboard"
fi

say "Cardboard installed."
echo "$launch"
echo "Agent setup (MCP): point your agent at \"$bin_dir/cardboard\" with args [\"mcp\"]."
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "Note: $bin_dir is not on your PATH. Add it to use the cardboard command." ;;
esac
