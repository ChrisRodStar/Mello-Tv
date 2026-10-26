#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=============================================="
echo " Packaging Mello TV v0.1.0-alpha.1 for macOS"
echo "=============================================="

# 1. Build Web Assets
echo "[1/6] Building Web Frontend assets..."
(cd "$ROOT_DIR/web" && bun run build)

# 2. Build Native Rust Server
echo "[2/6] Compiling stream-server (Rust release)..."
(cd "$ROOT_DIR" && cargo build --release -p stream-server)

# 3. Compile the same Icon Composer source used by iOS, including macOS appearances.
echo "[3/6] Compiling shared iOS/macOS app icon..."
ICON_BUILD_DIR="$(mktemp -d -t mello-app-icon)"
trap 'rm -rf "$ICON_BUILD_DIR"' EXIT
mkdir -p "$ICON_BUILD_DIR/compiled"
xcrun actool "$ROOT_DIR/ios/MelloTV/Resources/AppIcon.icon" \
    --compile "$ICON_BUILD_DIR/compiled" \
    --platform macosx --minimum-deployment-target 13.0 \
    --app-icon AppIcon --target-device mac \
    --output-partial-info-plist "$ICON_BUILD_DIR/icon-info.plist"
cp "$ICON_BUILD_DIR/compiled/AppIcon.icns" "$ROOT_DIR/macos/AppIcon.icns"

# 4. Compile the native Mello TV application.
echo "[4/6] Compiling Mello TV (SwiftUI + AppKit)..."
NATIVE_BUILD_DIR="$ROOT_DIR/target/macos"
mkdir -p "$NATIVE_BUILD_DIR"
SWIFT_SOURCES=()
while IFS= read -r source; do
    SWIFT_SOURCES+=("$source")
done < <(find "$ROOT_DIR/macos/MelloTV" -type f -name '*.swift' | sort)
swiftc -O -framework Cocoa -framework SwiftUI -target arm64-apple-macos13.0 \
    "${SWIFT_SOURCES[@]}" -o "$NATIVE_BUILD_DIR/MelloTV"

# 5. Assemble Mello TV.app Bundle
echo "[5/6] Assembling Mello TV.app bundle..."
DIST_DIR="$ROOT_DIR/dist/macos"
APP_DIR="$DIST_DIR/Mello TV.app"

rm -rf "$DIST_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

cp "$ROOT_DIR/macos/Info.plist" "$APP_DIR/Contents/Info.plist"
cp "$NATIVE_BUILD_DIR/MelloTV" "$APP_DIR/Contents/MacOS/MelloTV"
cp "$ROOT_DIR/target/release/stream-server" "$APP_DIR/Contents/MacOS/MelloTVServer"
cp "$ROOT_DIR/macos/AppIcon.icns" "$APP_DIR/Contents/Resources/AppIcon.icns"
cp "$ICON_BUILD_DIR/compiled/Assets.car" "$APP_DIR/Contents/Resources/Assets.car"
cp -R "$ROOT_DIR/web/dist" "$APP_DIR/Contents/Resources/web"

chmod +x "$APP_DIR/Contents/MacOS/MelloTV"
chmod +x "$APP_DIR/Contents/MacOS/MelloTVServer"

# Ad-hoc code sign for local macOS execution without gatekeeper prompt
codesign --force --deep --sign - "$APP_DIR"

# 6. Create Drag-and-Drop DMG Installer
echo "[6/6] Generating Mello-TV-0.1.0-alpha.1-macOS.dmg..."
DMG_STAGING="$DIST_DIR/dmg_staging"
mkdir -p "$DMG_STAGING"
cp -R "$APP_DIR" "$DMG_STAGING/Mello TV.app"
ln -s /Applications "$DMG_STAGING/Applications"

DMG_OUTPUT="$DIST_DIR/Mello-TV-0.1.0-alpha.1-macOS.dmg"
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
