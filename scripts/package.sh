#!/usr/bin/env bash
# Packages a release build into dist/. Run after:
#   pnpm -C ui install --frozen-lockfile && pnpm -C ui build
#   cargo build --release
#   cargo build --release --manifest-path desktop/Cargo.toml
# CEF_PATH must point at the Chromium runtime the desktop build used.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
version="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=macos ;;
  MINGW* | MSYS* | CYGWIN*) os=windows ;;
  *) echo "unsupported OS" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x64 ;;
  arm64 | aarch64) arch=arm64 ;;
  *) echo "unsupported architecture" >&2; exit 1 ;;
esac
# cef-dll-sys keeps the runtime either directly in CEF_PATH or in CEF_PATH/<version>/cef_<os>_<arch>.
cef="$(dirname "$(find "${CEF_PATH:?set CEF_PATH}" -maxdepth 4 -name archive.json | head -1)")"
bin="desktop/target/release"
name="cardboard-$os-$arch"
out="dist/$name"
rm -rf "$out" && mkdir -p "$out"

case "$os" in
  linux)
    cp "$bin/cardboard-desktop" target/release/cardboard "$out/"
    for f in "$cef"/*; do
      case "$(basename "$f")" in
        CMakeLists.txt | archive.json | cmake | include | libcef_dll) ;;
        *) cp -R "$f" "$out/" ;;
      esac
    done
    strip --strip-unneeded "$out/libcef.so" "$out/cardboard-desktop" "$out/cardboard" 2>/dev/null || true
    cp assets/cardboard.svg "$out/cardboard.svg"
    for size in 64 128 256 512; do cp "packaging/icons/cardboard-$size.png" "$out/cardboard-$size.png"; done
    tar -C dist -czf "dist/$name.tar.gz" "$name"
    ;;
  macos)
    app="$out/Cardboard.app"
    contents="$app/Contents"
    mkdir -p "$contents/MacOS" "$contents/Frameworks" "$contents/Resources"
    cp "$bin/cardboard-desktop" "$contents/MacOS/Cardboard"
    # Separate name: the default macOS filesystem ignores case, so "cardboard" would clash with "Cardboard".
    cp target/release/cardboard "$contents/MacOS/cardboard-cli"
    framework="$(find "$cef" -maxdepth 3 -name 'Chromium Embedded Framework.framework' -type d | head -1)"
    cp -R "$framework" "$contents/Frameworks/"
    iconset="$(mktemp -d)/cardboard.iconset" && mkdir -p "$iconset"
    for size in 16 32 128 256 512; do
      cp "packaging/icons/cardboard-$size.png" "$iconset/icon_${size}x${size}.png"
      cp "packaging/icons/cardboard-$((size * 2)).png" "$iconset/icon_${size}x${size}@2x.png"
    done
    iconutil -c icns "$iconset" -o "$contents/Resources/cardboard.icns"
    plist() { # plist <path> <executable> <bundle id> <is helper>
      cat > "$1" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>$2</string>
  <key>CFBundleDisplayName</key><string>$2</string>
  <key>CFBundleExecutable</key><string>$2</string>
  <key>CFBundleIdentifier</key><string>$3</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleIconFile</key><string>cardboard</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>LSEnvironment</key><dict><key>MallocNanoZone</key><string>0</string></dict>
  $( [ "$4" = 1 ] && echo '<key>LSUIElement</key><string>1</string>' )
</dict></plist>
PLIST
    }
    plist "$contents/Info.plist" Cardboard dev.goerll.cardboard 0
    # Chromium looks for "<app> Helper*.app" next to the framework.
    for suffix in "" " (GPU)" " (Renderer)" " (Plugin)" " (Alerts)"; do
      helper="$contents/Frameworks/Cardboard Helper$suffix.app/Contents"
      mkdir -p "$helper/MacOS"
      cp "$bin/cardboard_helper" "$helper/MacOS/Cardboard Helper$suffix"
      id_suffix="$(echo "$suffix" | tr -d ' ()' | tr '[:upper:]' '[:lower:]')"
      plist "$helper/Info.plist" "Cardboard Helper$suffix" "dev.goerll.cardboard.helper${id_suffix:+.$id_suffix}" 1
    done
    # Ad-hoc signature: Apple Silicon refuses to run unsigned code at all.
    codesign --force --deep --sign - "$app"
    (cd "$out" && ditto -c -k --keepParent Cardboard.app "../$name.zip")
    ;;
  windows)
    cp "$bin/cardboard-desktop.exe" "$out/"
    cp target/release/cardboard.exe "$out/cardboard.exe"
    for f in "$cef"/*; do
      case "$(basename "$f")" in
        CMakeLists.txt | archive.json | cmake | include | libcef_dll | *.lib) ;;
        *) cp -R "$f" "$out/" ;;
      esac
    done
    cp packaging/icons/cardboard.ico "$out/"
    (cd dist && 7z a -tzip -mx=7 "$name.zip" "$name" >/dev/null)
    ;;
esac
echo "dist/$name"
