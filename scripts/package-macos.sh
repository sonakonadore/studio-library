#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/.."
target=aarch64-apple-darwin
app="$PWD/dist/Studio Library.app"
cargo build --locked --release --target "$target"

mkdir -p "$app/Contents/MacOS"
/usr/bin/install -m 755 "${CARGO_TARGET_DIR:-target}/$target/release/studio-library" "$app/Contents/MacOS/studio-library"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>ko</string>
  <key>CFBundleDisplayName</key><string>스튜디오 라이브러리</string>
  <key>CFBundleExecutable</key><string>studio-library</string>
  <key>CFBundleIdentifier</key><string>local.studio-library.mac</string>
  <key>CFBundleName</key><string>스튜디오 라이브러리</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
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
/usr/bin/file "$app/Contents/MacOS/studio-library"
printf '%s\n' "$app"
