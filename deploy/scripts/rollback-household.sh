#!/bin/sh
#
#  rollback-household.sh
#  Stream TV Deployment
#
#  Created by ChrisRodStar.
#
#  Summary:
#  Reverts the household access update on the server. Restores previous API and web
#  components while preserving newly recorded watch progress and profile data.
#
set -eu

DEPLOY_ROOT="/opt/stream-tv"
[ -d "$DEPLOY_ROOT" ] || DEPLOY_ROOT="/opt/stream-tv"
CURRENT_RELEASE="$DEPLOY_ROOT/current"
BACKUP_DIR="$DEPLOY_ROOT/backups/pre-household-20261017"

cd "$CURRENT_RELEASE"

umask 077
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
retained_dir="$DEPLOY_ROOT/backups/household-before-rollback-$timestamp"
mkdir -p "$retained_dir"

# Step 1: Gracefully stop the Application container
echo "Stopping Application container..."
docker compose stop --timeout 60 application

# Step 2: Preserve newly recorded watch progress and profile data before rolling back
echo "Preserving latest runtime state in $retained_dir..."
cp -a "$DEPLOY_ROOT/state/application" "$retained_dir/state"
cp -a "$DEPLOY_ROOT/private/application" "$retained_dir/private"

# Step 3: Revert API binary pointer to previous revision
previous_api_version="$(cat "$BACKUP_DIR/api-version.txt")"
ln -s "$previous_api_version" components/api/rollback-next
mv -Tf components/api/rollback-next components/api/current

# Step 4: Revert static web bundle and source version registry
cp -a "$BACKUP_DIR/web/." artifacts/web/
cp "$BACKUP_DIR/source-versions.json" source-versions.json

# Step 5: Restart the Application container with the previous release
docker compose up -d --no-build application

printf 'Previous API/web selected; current personal data retained. Previous version requires sign-in.\n'
