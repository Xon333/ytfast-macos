#!/bin/bash
# Build a native app without bundling Homebrew's dependency tree or changing the host.
set -euo pipefail
[[ "$(uname -s)" == Darwin ]] || { echo 'Build this app on macOS.' >&2; exit 1; }
repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"
for tool in cargo rustc cmake codesign ditto plutil; do
  command -v "$tool" >/dev/null || { echo "Missing build tool: $tool" >&2; exit 1; }
done
target="${CARGO_BUILD_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
case "$target" in
  aarch64-apple-darwin) arch=arm64 ;;
  x86_64-apple-darwin) arch=x86_64 ;;
  *) echo "Unsupported Mac target: $target" >&2; exit 1 ;;
esac
export MACOSX_DEPLOYMENT_TARGET=13.0
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
cargo build --locked --release --target "$target"
target_dir="${CARGO_TARGET_DIR:-$repo/target}"
mkdir -p "$repo/dist" "$repo/target"
staging="$(mktemp -d "$repo/target/macos-package.XXXXXX")"
trap 'rm -rf "$staging"' EXIT
app="$staging/YTfast.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 "$target_dir/$target/release/ytfast" "$app/Contents/MacOS/ytfast"
install -m 644 LICENSE "$app/Contents/Resources/LICENSE"
install -m 644 assets/icons/LICENSE.txt "$app/Contents/Resources/Lucide-LICENSE.txt"
version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' Cargo.toml | head -1)"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>io.github.xon333.ytfast</string>
<key>CFBundleName</key><string>YTfast</string>
<key>CFBundleDisplayName</key><string>YTfast</string>
<key>CFBundleExecutable</key><string>ytfast</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>$version</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
PLIST
plutil -lint "$app/Contents/Info.plist"
codesign --force --sign - "$app"
codesign --verify --strict "$app"
file "$app/Contents/MacOS/ytfast"
archive="$repo/dist/ytfast-macos-$arch.zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "$archive"
# This directory is generated output, never an installed app.
rm -rf "$repo/dist/YTfast.app"
mv "$app" "$repo/dist/YTfast.app"
printf '\nBuilt: %s\nArchive: %s\n' "$repo/dist/YTfast.app" "$archive"
