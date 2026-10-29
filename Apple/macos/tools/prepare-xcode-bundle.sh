#!/bin/bash
set -euo pipefail

# Xcode does not inherit the interactive shell's Cargo/Bun paths.
export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
ROOT_DIR="$(cd "$SRCROOT/../.." && pwd)"
for tool in cargo bun; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: $tool is required to build Mello TV. Install it, then build again." >&2
        exit 1
    fi
done

# Keep native UI iteration on Cargo's incremental development build.
RUST_TARGET_DIR="$ROOT_DIR/Local/build/target"
CARGO_OPTIONS=(--locked -p stream-server --target-dir "$RUST_TARGET_DIR")
SERVER_PROFILE=debug
if [[ "$CONFIGURATION" == Release ]]; then
    CARGO_OPTIONS+=(--release)
    SERVER_PROFILE=release
fi
cd "$ROOT_DIR"
cargo build "${CARGO_OPTIONS[@]}"

WEB_OUTPUT="$DERIVED_FILE_DIR/mellotv-web"
cd "$ROOT_DIR/web"
bun install --frozen-lockfile
bun run vite build --outDir "$WEB_OUTPUT" --emptyOutDir

MACOS_DIR="$TARGET_BUILD_DIR/$EXECUTABLE_FOLDER_PATH"
RESOURCES_DIR="$TARGET_BUILD_DIR/$UNLOCALIZED_RESOURCES_FOLDER_PATH"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
ditto "$RUST_TARGET_DIR/$SERVER_PROFILE/stream-server" "$MACOS_DIR/MelloTVServer"
# Replace the generated web folder so removed assets do not survive rebuilds.
if [[ -d "$RESOURCES_DIR/web" ]]; then
    rm -rf "$RESOURCES_DIR/web"
fi
ditto "$WEB_OUTPUT" "$RESOURCES_DIR/web"
chmod +x "$MACOS_DIR/MelloTVServer"

# Wasmtime needs executable memory in the bundled child process.
if [[ "${CODE_SIGNING_ALLOWED:-YES}" != NO ]]; then
    /usr/bin/codesign --force --sign "${EXPANDED_CODE_SIGN_IDENTITY:--}" \
        --options runtime --entitlements "$SRCROOT/MelloTVServer.entitlements" \
        "$MACOS_DIR/MelloTVServer"
fi
