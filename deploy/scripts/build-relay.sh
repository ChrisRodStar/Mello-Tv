#!/bin/sh
#
#  build-relay.sh
#  Stream TV Deployment
#
#  Created by ChrisRodStar.
#
#  Summary:
#  Compiles the production Linux ARM64 relay binary directly from the single source
#  of truth (relay/) using an isolated rust:1.99-bookworm Docker container.
#
set -eu

script_dir="$(cd "$(dirname "$0")" && pwd)"
deploy_dir="$(cd "$script_dir/.." && pwd)"
workspace_dir="$(cd "$deploy_dir/.." && pwd)"

# Prefer new role-based folder 'relay/', fallback to 'family-stream-relay/'
if [ -f "$workspace_dir/relay/Cargo.toml" ]; then
    relay_source="$workspace_dir/relay"
elif [ -f "$workspace_dir/family-stream-relay/Cargo.toml" ]; then
    relay_source="$workspace_dir/family-stream-relay"
else
    echo "Error: relay source directory with Cargo.toml not found in $workspace_dir" >&2
    exit 1
fi

artifacts_dir="$deploy_dir/artifacts"
mkdir -p "$artifacts_dir"

echo "Building relay using rust:1.99-bookworm from source: $relay_source"

docker run --rm \
    --user "$(id -u):$(id -g)" \
    -v "$relay_source:/src:ro" \
    -v "$artifacts_dir:/out" \
    -w /src \
    rust:1.99-bookworm \
    sh -c "cargo build --locked --release --target-dir /tmp/target && cp /tmp/target/release/*relay /out/stv-relay"

echo "Relay binary successfully built and placed at: $artifacts_dir/stv-relay"
