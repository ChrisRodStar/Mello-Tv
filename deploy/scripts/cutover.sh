#!/bin/sh
#
#  cutover.sh
#  Stream TV Deployment
#
#  Created by ChrisRodStar.
#
#  Summary:
#  Executes production cutover on the server. Stops legacy proof-of-concept containers,
#  archives legacy volume data, sets WireGuard permissions, activates V2 release,
#  and points Tailscale Serve routes to the 3-container production stack.
#
set -eu

DEPLOY_ROOT="/opt/stream-tv"
RELEASE_DIR="$DEPLOY_ROOT/releases/20261017"
BACKUP_DIR="$DEPLOY_ROOT/backups/pre-v2"

cd "$RELEASE_DIR"

# Step 1: Disable restart policies and gracefully stop legacy proof-of-concept containers
echo "Stopping legacy containers..."
docker update --restart=no deploy-family-stream-1 deploy-vidcore-resolver-1 deploy-mullvad-1 >/dev/null
docker stop --time 60 deploy-family-stream-1 deploy-vidcore-resolver-1 deploy-mullvad-1 >/dev/null

# Step 2: Archive legacy application data to the backup directory
umask 077
archive_path="$BACKUP_DIR/legacy-data.tar"
if [ -e "$archive_path" ]; then
    archive_path="$BACKUP_DIR/legacy-data-$(date -u +%Y%m%dT%H%M%SZ).tar"
fi

docker cp deploy-family-stream-1:/app/data/. - > "$archive_path"
sha256sum "$archive_path" "$BACKUP_DIR/images.tar.gz" >> "$BACKUP_DIR/archive-checksums.txt"

# Step 3: Restrict WireGuard private key permissions to root-only
docker run --rm --network none \
    -v "$DEPLOY_ROOT/private:/private" \
    debian:bookworm-slim \
    chown 0:0 /private/streaming/wireguard.conf

# Step 4: Atomically activate the 20261017 release directory symlink
ln -sfn releases/20261017 "$DEPLOY_ROOT/current"

# Step 5: Launch the 3-container production stack
docker compose up -d --no-build

# Step 6: Route private incoming traffic via Tailscale Serve
# Note: Targets include original prefixes because Serve strips the matched mount path.
tailscale serve --bg --yes http://127.0.0.1:8090
tailscale serve --bg --yes --set-path /api http://127.0.0.1:8090/api
tailscale serve --bg --yes --set-path /stream http://127.0.0.1:8095/stream

printf 'Cutover commands completed. Manual acceptance remains pending.\n'
