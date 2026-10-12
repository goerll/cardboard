#!/usr/bin/env bash
set -euo pipefail
cardboard_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$cardboard_root"
# Share the browser runtime across Cargo's check/build artifact directories.
export CEF_PATH="${CEF_PATH:-${XDG_CACHE_HOME:-$HOME/.cache}/cardboard/cef}"
# Rebuild the editor UI when its sources are newer than the last build.
if [[ -f ui/package.json ]]; then
  [[ -d ui/node_modules ]] || npm --prefix ui install
  if [[ ! -f ui/dist/index.html ]] || [[ -n "$(find ui/src ui/index.html ui/package.json -newer ui/dist/index.html -print -quit 2>/dev/null)" ]]; then
    npm --prefix ui run build
  fi
fi
cargo build --manifest-path desktop/Cargo.toml -j 8
# Default: the file library (~/Documents/Cardboard or $CARDBOARD_LIBRARY). Pass --library PATH to override.
exec "$cardboard_root/desktop/target/debug/cardboard-desktop" "$@"
