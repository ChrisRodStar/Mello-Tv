#!/bin/sh
#
#  rollback.sh
#  Stream TV Deployment
#
#  Created by ChrisRodStar.
#
#  Summary:
#  Emergency rollback procedure on the server. Stops the V2 container stack without data
#  loss, restores legacy container restart policies, and resets Tailscale Serve to port 3000.
#
set -eu

RELEASE_DIR="/opt/stream-tv/releases/20261017"
[ -d "$RELEASE_DIR" ] || RELEASE_DIR="/opt/stream-tv/releases/20261017"
cd "$RELEASE_DIR"

# Step 1: Gracefully stop V2 containers while preserving their data volumes
docker compose stop --timeout 150

# Step 2: Restore legacy container restart policies from saved backup state
python3 - <<'RESTORE'
import json
from pathlib import Path
import subprocess

backup_path = Path(RELEASE_DIR).parents[0] / 'backups/pre-v2/containers.json'
containers = json.loads(backup_path.read_text())

for container in containers:
    container_name = container['Name'].lstrip('/')
    policy = container['HostConfig']['RestartPolicy']
    policy_name = policy['Name'] or 'no'

    if policy_name == 'on-failure' and policy.get('MaximumRetryCount'):
        policy_name = f"{policy_name}:{policy['MaximumRetryCount']}"

    subprocess.run(
        ['docker', 'update', f'--restart={policy_name}', container_name],
        check=True,
        stdout=subprocess.DEVNULL
    )
RESTORE

# Step 3: Start legacy containers
docker start deploy-mullvad-1 deploy-vidcore-resolver-1 deploy-family-stream-1 >/dev/null

# Step 4: Verify and restore Tailscale Serve routing to legacy port 3000 if needed
if python3 - <<'ROUTING'
import json
from pathlib import Path
import subprocess

backup_root = Path('/opt/stream-tv/backups/pre-v2')
if not backup_root.exists():
    backup_root = Path('/opt/stream-tv/backups/pre-v2')
original_routing = json.loads((backup_root / 'serve.json').read_text())
current_routing = json.loads(subprocess.check_output(['tailscale', 'serve', 'status', '--json'], text=True))

raise SystemExit(0 if original_routing == current_routing else 1)
ROUTING
then
    : # Routing is already identical to original; no updates needed
else
    tailscale serve --bg --yes --set-path /api off
    tailscale serve --bg --yes --set-path /stream off
    tailscale serve --bg --yes http://127.0.0.1:3000
fi

printf 'Legacy deployment selected. V2 data and all rollback archives retained.\n'
