#!/bin/sh
#
#  finish-routing.sh
#  Stream TV Deployment
#
#  Created by ChrisRodStar.
#
#  Summary:
#  Configures host-level Tailscale Serve routing, mapping incoming HTTPS
#  traffic for the web UI (/), Go API (/api), and Rust relay (/stream).
#
set -eu

APPLICATION_URL="http://127.0.0.1:8090"
RELAY_URL="http://127.0.0.1:8095"

echo "Configuring Tailscale Serve routes..."

# Route root path (/) to the Application container (Web frontend)
tailscale serve --bg --yes "$APPLICATION_URL"

# Route /api path to the Application container (Go API backend)
tailscale serve --bg --yes --set-path /api "$APPLICATION_URL/api"

# Route /stream path directly to the Streaming container (Rust Relay)
tailscale serve --bg --yes --set-path /stream "$RELAY_URL/stream"

printf 'Private routing configured successfully.\n'
