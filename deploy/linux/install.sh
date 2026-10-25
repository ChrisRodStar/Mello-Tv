#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

echo "Installing Stream TV background service for Linux..."

BIN_DIR="$HOME/.local/bin"
DATA_DIR="$HOME/.local/share/stream"
SYSTEMD_USER_DIR="$HOME/.config/systemd/user"

mkdir -p "$BIN_DIR" "$DATA_DIR/data" "$DATA_DIR/web" "$SYSTEMD_USER_DIR"

# Install binary
if [[ -f "$ROOT_DIR/target/release/stream-server" ]]; then
    install -m 755 "$ROOT_DIR/target/release/stream-server" "$BIN_DIR/stream-server"
else
    echo "Error: target/release/stream-server not found. Run 'cargo build --release -p stream-server' first."
    exit 1
fi

# Install web assets
if [[ -d "$ROOT_DIR/web/dist" ]]; then
    cp -r "$ROOT_DIR/web/dist/"* "$DATA_DIR/web/"
fi

# Install systemd service
cp "$SCRIPT_DIR/stream.service" "$SYSTEMD_USER_DIR/stream.service"

systemctl --user daemon-reload
systemctl --user enable --now stream.service

echo "Stream service installed and running in the background!"
echo "Check status: systemctl --user status stream.service"
echo "View logs:    journalctl --user -u stream.service -f"
echo "Access app:   http://localhost:8080"
