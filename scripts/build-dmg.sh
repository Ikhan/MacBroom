#!/bin/sh
# Builds dist/macbroom-<version>.dmg containing macbroom.app.
# Double-clicking the app opens the interactive UI in Terminal.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
DIST="$ROOT/dist"
WORK="$DIST/work"
APP="$WORK/stage/macbroom.app"
DMG="$DIST/macbroom-$VERSION.dmg"

echo "==> Building release binary"
cargo build --release --manifest-path "$ROOT/Cargo.toml"

echo "==> Assembling app bundle"
rm -rf "$WORK" "$DMG"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$WORK/icon.iconset"
cp "$ROOT/target/release/macbroom" "$APP/Contents/Resources/macbroom"

cat > "$APP/Contents/MacOS/macbroom-launcher" <<'SH'
#!/bin/sh
# Opens the bundled binary in a new Terminal window (no automation permission needed).
exec /usr/bin/open -a Terminal "$(cd "$(dirname "$0")/../Resources" && pwd)/macbroom"
SH
chmod +x "$APP/Contents/MacOS/macbroom-launcher"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>macbroom</string>
  <key>CFBundleDisplayName</key><string>macbroom</string>
  <key>CFBundleIdentifier</key><string>com.imrankhan.macbroom</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>macbroom-launcher</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

echo "==> Rendering icon"
swift "$ROOT/scripts/make-icon.swift" "$WORK/icon-1024.png"
for s in 16 32 128 256 512; do
  sips -z $s $s "$WORK/icon-1024.png" --out "$WORK/icon.iconset/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d "$WORK/icon-1024.png" --out "$WORK/icon.iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$WORK/icon.iconset" -o "$APP/Contents/Resources/AppIcon.icns"

echo "==> Ad-hoc signing"
codesign --force -s - "$APP/Contents/Resources/macbroom"
codesign --force -s - "$APP"

echo "==> Creating DMG"
ln -s /Applications "$WORK/stage/Applications"
cat > "$WORK/stage/READ ME.txt" <<TXT
macbroom $VERSION: fast, low-memory Mac cleaner

1. Drag macbroom.app into Applications.
2. Double-click it; it opens in Terminal. Press ? for keys.
3. Optional, for Safari/Mail/Trash scanning: System Settings > Privacy &
   Security > Full Disk Access > enable Terminal.

Command-line use (optional):
  sudo ln -sf /Applications/macbroom.app/Contents/Resources/macbroom /usr/local/bin/macbroom
  macbroom --help
TXT
hdiutil create -volname "macbroom $VERSION" -srcfolder "$WORK/stage" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$WORK"
echo "==> Done: $DMG ($(du -h "$DMG" | cut -f1))"
