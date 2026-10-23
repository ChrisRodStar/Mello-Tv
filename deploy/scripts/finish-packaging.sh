#!/bin/sh
#
#  finish-packaging.sh
#  Stream TV Deployment
#
#  Created by ChrisRodStar.
#
#  Summary:
#  Helper script on the server that extracts the compiled relay binary from the builder
#  container, triggers component activation, and builds production Docker images.
#
set -eu

RELEASE_DIR="/opt/stream-tv/releases/20261017"
BUILDER_IMAGE="stream-tv/relay-build:20261017"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

cd "$RELEASE_DIR"

# Step 1: Create a temporary container instance to extract the compiled relay binary
echo "Extracting compiled relay binary from $BUILDER_IMAGE..."
container_id=$(docker create "$BUILDER_IMAGE")
trap 'docker rm "$container_id" >/dev/null' EXIT

mkdir -p artifacts
docker cp "$container_id:/src/target/release/stv-relay" artifacts/stv-relay

# Step 2: Organize and activate component binaries into their versioned directories
python3 "$SCRIPT_DIR/activate-components.py"

# Step 3: Build production Docker images for application and streaming containers
echo "Building production container images..."
docker compose build application streaming

echo "Packaging completed successfully."
