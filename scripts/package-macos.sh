#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/.."
target=aarch64-apple-darwin
bundle="$PWD/dist/Studio Library.app"
cargo build --locked --release --target "$target"

work="$(mktemp -d "${TMPDIR:-/tmp}/studio-library-bundle.XXXXXX")"
trap 'rm -rf "$work"' EXIT
app="$work/Studio Library.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
/usr/bin/install -m 755 "${CARGO_TARGET_DIR:-target}/$target/release/studio-library" "$app/Contents/MacOS/studio-library"
/usr/bin/install -m 644 assets/StudioLibrary.icns "$app/Contents/Resources/StudioLibrary.icns"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>Studio Library</string>
  <key>CFBundleExecutable</key><string>studio-library</string>
  <key>CFBundleIconFile</key><string>StudioLibrary.icns</string>
  <key>CFBundleIdentifier</key><string>local.studio-library.mac</string>
  <key>CFBundleName</key><string>Studio Library</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>11</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
/usr/bin/plutil -lint "$app/Contents/Info.plist"
/usr/bin/xattr -cr "$app"
/usr/bin/codesign --force --sign - "$app"
/usr/bin/codesign --verify --strict --verbose=2 "$app"

mkdir -p "$PWD/dist"
if [[ -e "$bundle" ]]; then
  mv "$bundle" "$work/Previous.app"
fi
if ! mv "$app" "$bundle"; then
  if [[ -e "$work/Previous.app" ]] && ! mv "$work/Previous.app" "$bundle"; then
    trap - EXIT
    printf 'Previous bundle preserved at %s\n' "$work/Previous.app" >&2
  fi
  exit 1
fi
/usr/bin/file "$bundle/Contents/MacOS/studio-library"
printf '%s\n' "$bundle"
