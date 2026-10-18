#!/usr/bin/env bash
# End-to-end check of the self-updater on Linux, without GitHub or a desktop build:
#   builds this version and a fake "next patch" CLI, packages the next one in the
#   scripts/package.sh layout (stub desktop binary), signs it with a throwaway key, serves a fake
#   releases API locally, installs this version like site/install.sh and runs `cardboard update`.
# Needs minisign. Debug builds only: the API URL and test key overrides don't exist in release.
#   scripts/e2e_update.sh                 # throwaway key
#   RELEASE_KEY=path scripts/e2e_update.sh  # also sign one run with the real key (embedded key path)
set -euo pipefail
repo="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
target="${CARGO_TARGET_DIR:-$repo/target}"
work="$(mktemp -d "${TMPDIR:-/tmp}/cardboard-e2e.XXXXXX")"
pids=()
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done; rm -rf "$work"; }
trap cleanup EXIT
step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

old="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/Cargo.toml" | head -1)"
IFS=. read -r major minor patch <<<"${old%%-*}"
new="$major.$minor.$((patch + 1))"
asset=cardboard-linux-x64.tar.gz

step "build $old and a fake $new"
cargo build -q --manifest-path "$repo/Cargo.toml"
mkdir -p "$work/old" "$work/new" "$work/src"
cp "$target/debug/cardboard" "$work/old/cardboard"
cp -R "$repo/Cargo.toml" "$repo/Cargo.lock" "$repo/src" "$repo/packaging" "$repo/guides" "$work/src/"
sed -i "0,/^version = \"$old\"/s//version = \"$new\"/" "$work/src/Cargo.toml"
cargo build -q --offline --manifest-path "$work/src/Cargo.toml"
cp "$target/debug/cardboard" "$work/new/cardboard"
"$work/old/cardboard" --version
"$work/new/cardboard" --version

# A desktop stand-in: records its version and arguments when launched.
stub() { printf '#!/bin/sh\necho "%s $*" > "%s/launched"\n' "$1" "$work"; }
layout() { # layout <dir> <cli> <version>
  mkdir -p "$1"
  cp "$2" "$1/cardboard"
  stub "$3" > "$1/cardboard-desktop" && chmod +x "$1/cardboard-desktop"
  : > "$1/libcef.so"
  cp "$repo/assets/cardboard.svg" "$1/cardboard.svg"
  for size in 64 128 256 512; do cp "$repo/packaging/icons/cardboard-$size.png" "$1/"; done
}
mkdir -p "$work/pkg" "$work/srv"
layout "$work/pkg/cardboard-linux-x64" "$work/new/cardboard" "$new"
tar -C "$work/pkg" -czf "$work/good.tar.gz" cardboard-linux-x64
minisign -G -W -f -p "$work/test.pub" -s "$work/test.key" >/dev/null
sign() { minisign -S -s "${3:-$work/test.key}" -m "$1" -x "$1.minisig" -t "cardboard file:$asset version:$2" >/dev/null; }

port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
python3 -m http.server --bind 127.0.0.1 "$port" --directory "$work/srv" >/dev/null 2>&1 &
pids+=($!)
for _ in $(seq 50); do python3 -c "import socket; socket.create_connection(('127.0.0.1', $port))" 2>/dev/null && break; sleep 0.1; done
publish() { # publish <archive> [signature]: the fake GitHub "latest release"
  rm -f "$work/srv/"*
  cp "$1" "$work/srv/$asset"
  local assets
  assets="{\"name\":\"$asset\",\"size\":$(stat -c %s "$1"),\"browser_download_url\":\"http://127.0.0.1:$port/$asset\"}"
  if [ -n "${2:-}" ]; then
    cp "$2" "$work/srv/$asset.minisig"
    assets="$assets,{\"name\":\"$asset.minisig\",\"size\":1,\"browser_download_url\":\"http://127.0.0.1:$port/$asset.minisig\"}"
  fi
  echo "{\"tag_name\":\"v$new\",\"html_url\":\"https://github.com/goerll/cardboard/releases/tag/v$new\",\"assets\":[$assets]}" > "$work/srv/latest.json"
}

# Everything below runs in a throwaway home, so the real app, socket and install are never seen.
export HOME="$work/home" XDG_DATA_HOME="$work/data" XDG_RUNTIME_DIR="$work/run"
export CARDBOARD_UPDATE_API="http://127.0.0.1:$port/latest.json" CARDBOARD_UPDATE_TEST_KEY="$work/test.pub"
unset CARDBOARD_NO_UPDATE
mkdir -p "$HOME/.local/bin" "$XDG_RUNTIME_DIR"
app="$XDG_DATA_HOME/cardboard/app"
cli="$HOME/.local/bin/cardboard"
install_old() { # what site/install.sh leaves behind
  rm -rf "$XDG_DATA_HOME/cardboard" "$work/launched"
  layout "$app" "$work/old/cardboard" "$old"
  ln -sf "$app/cardboard" "$cli"
  ln -sf "$app/cardboard-desktop" "$HOME/.local/bin/cardboard-desktop"
  mkdir -p "$XDG_DATA_HOME/icons/hicolor/scalable/apps"
  cp "$app/cardboard.svg" "$XDG_DATA_HOME/icons/hicolor/scalable/apps/cardboard.svg"
}
expect_version() { [ "$("$cli" --version)" = "cardboard $1" ] || fail "expected $1, got $("$cli" --version)"; echo "cardboard --version: $("$cli" --version)"; }
run_update() { "$cli" update "$@" 2>&1 | sed 's/^/  | /'; return "${PIPESTATUS[0]}"; }

step "dev builds never update themselves"
cp "$work/good.tar.gz" "$work/signed.tar.gz" && sign "$work/signed.tar.gz" "$new"
publish "$work/signed.tar.gz" "$work/signed.tar.gz.minisig"
"$work/old/cardboard" update 2>&1 | sed 's/^/  | /' && fail "a dev build updated" || true

step "unsigned release: not available yet"
install_old
publish "$work/good.tar.gz"
run_update
expect_version "$old"

step "tampered archive: refused"
install_old
cp "$work/good.tar.gz" "$work/tampered.tar.gz" && sign "$work/tampered.tar.gz" "$new"
printf 'x' >> "$work/tampered.tar.gz"
publish "$work/tampered.tar.gz" "$work/tampered.tar.gz.minisig"
run_update && fail "a tampered archive was accepted"
expect_version "$old"

step "signature for another version: refused"
install_old
cp "$work/good.tar.gz" "$work/replay.tar.gz" && sign "$work/replay.tar.gz" "$old"
publish "$work/replay.tar.gz" "$work/replay.tar.gz.minisig"
run_update && fail "a replayed signature was accepted"
expect_version "$old"

step "corrupt archive (validly signed): install untouched"
install_old
head -c 4000 "$work/good.tar.gz" > "$work/corrupt.tar.gz" && sign "$work/corrupt.tar.gz" "$new"
publish "$work/corrupt.tar.gz" "$work/corrupt.tar.gz.minisig"
run_update && fail "a corrupt archive was installed"
expect_version "$old"
[ -x "$app/cardboard-desktop" ] || fail "install damaged"
ls -A "$XDG_DATA_HOME/cardboard"

step "good release: cardboard update"
install_old
publish "$work/signed.tar.gz" "$work/signed.tar.gz.minisig"
run_update --check
expect_version "$old"
run_update
expect_version "$new"
ls -A "$XDG_DATA_HOME/cardboard"
[ ! -e "$XDG_DATA_HOME/cardboard/.app.old" ] && [ ! -e "$XDG_DATA_HOME/cardboard/.app.new" ] || fail "leftovers"
run_update

step "app running: stage, then the helper swaps after it quits and relaunches it"
install_old
# Stand-in for the desktop app: owns the socket and reports itself as the app.
python3 - "$XDG_RUNTIME_DIR/cardboard.sock" <<'PY' &
import json, os, socket, sys
s = socket.socket(socket.AF_UNIX); s.bind(sys.argv[1]); s.listen()
while True:
    c, _ = s.accept(); c.recv(4096)
    c.sendall((json.dumps({"mode": "app", "pid": os.getpid(), "version": "old"}) + "\n").encode()); c.close()
PY
fake_app=$!
pids+=("$fake_app")
sleep 0.5
run_update
expect_version "$old"
ls -A "$XDG_DATA_HOME/cardboard"
# What the desktop does on quit after "Restart": a temp copy of the CLI waits for it, swaps, relaunches.
cp "$app/cardboard" "$work/helper"
"$work/helper" update --apply --root "$app" --wait-pid "$fake_app" --relaunch -- --library "$HOME/lib" &
helper=$!
sleep 1
kill -0 "$helper" || fail "the helper didn't wait for the app"
expect_version "$old"
kill "$fake_app"
wait "$helper"
sleep 0.5
expect_version "$new"
echo "relaunched: $(cat "$work/launched")"
grep -q "^$new --library $HOME/lib" "$work/launched" || fail "not relaunched with the new version"

step "CARDBOARD_NO_UPDATE"
install_old
CARDBOARD_NO_UPDATE=1 "$cli" update 2>&1 | sed 's/^/  | /' && fail "updated despite CARDBOARD_NO_UPDATE" || true
expect_version "$old"

if [ -n "${RELEASE_KEY:-}" ]; then
  step "signed with the release key, verified with the embedded key"
  install_old
  cp "$work/good.tar.gz" "$work/release.tar.gz" && sign "$work/release.tar.gz" "$new" "$RELEASE_KEY"
  publish "$work/release.tar.gz" "$work/release.tar.gz.minisig"
  run_update && fail "the throwaway key accepted a release-key signature"
  (unset CARDBOARD_UPDATE_TEST_KEY; run_update)
  expect_version "$new"
fi

step "doctor"
"$cli" doctor 2>&1 | grep -E "version|updates" | sed 's/^/  | /'
printf '\n\033[1mUpdate e2e passed\033[0m\n'
