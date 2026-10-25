#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=============================================="
echo " Packaging Mello TV v0.5.0 for macOS"
echo "=============================================="

# 1. Build Web Assets
echo "[1/6] Building Web Frontend assets..."
(cd "$ROOT_DIR/web" && bun run build)

# 2. Build Native Rust Server
echo "[2/6] Compiling stream-server (Rust release)..."
(cd "$ROOT_DIR" && cargo build --release -p stream-server)

# 3. Generate AppIcon.icns if needed
if [[ ! -f "$ROOT_DIR/macos/AppIcon.icns" ]]; then
    echo "[3/6] Generating AppIcon.icns..."
    swift "$ROOT_DIR/macos/tools/generate-icon.swift" "$ROOT_DIR/macos/Stream.iconset" "$ROOT_DIR/macos/AppIcon.icns"
else
    echo "[3/6] AppIcon.icns exists, skipping generation."
fi

# 4. Compile Native Swift Menu Bar App
echo "[4/6] Compiling StreamMenuBar (Swift AppKit)..."
swiftc -O -framework Cocoa -target arm64-apple-macos13.0 \
    "$ROOT_DIR/macos/StreamMenuBar/main.swift" \
    -o "$ROOT_DIR/macos/StreamMenuBar/StreamMenuBar"

# 5. Assemble Mello TV.app Bundle
echo "[5/6] Assembling Mello TV.app bundle..."
DIST_DIR="$ROOT_DIR/dist/macos"
APP_DIR="$DIST_DIR/Mello TV.app"

rm -rf "$DIST_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

cp "$ROOT_DIR/macos/Info.plist" "$APP_DIR/Contents/Info.plist"
cp "$ROOT_DIR/macos/StreamMenuBar/StreamMenuBar" "$APP_DIR/Contents/MacOS/StreamMenuBar"
cp "$ROOT_DIR/target/release/stream-server" "$APP_DIR/Contents/MacOS/stream-server"
cp "$ROOT_DIR/macos/AppIcon.icns" "$APP_DIR/Contents/Resources/AppIcon.icns"
cp -R "$ROOT_DIR/web/dist" "$APP_DIR/Contents/Resources/web"

chmod +x "$APP_DIR/Contents/MacOS/StreamMenuBar"
chmod +x "$APP_DIR/Contents/MacOS/stream-server"

# Ad-hoc code sign for local macOS execution without gatekeeper prompt
codesign --force --deep --sign - "$APP_DIR"

# 6. Create Drag-and-Drop DMG Installer
echo "[6/6] Generating Mello-TV-0.5.0-macOS.dmg..."
DMG_STAGING="$DIST_DIR/dmg_staging"
mkdir -p "$DMG_STAGING"
cp -R "$APP_DIR" "$DMG_STAGING/Mello TV.app"
ln -s /Applications "$DMG_STAGING/Applications"

DMG_OUTPUT="$DIST_DIR/Mello-TV-0.5.0-macOS.dmg"
hdiutil create -volname "Mello TV" \
    -srcfolder "$DMG_STAGING" \
    -ov -format UDZO \
    "$DMG_OUTPUT"

rm -rf "$DMG_STAGING"

echo "=============================================="
echo " Build & Packaging Complete!"
echo " App Bundle: $APP_DIR"
echo " DMG Image:  $DMG_OUTPUT"
echo "=============================================="
