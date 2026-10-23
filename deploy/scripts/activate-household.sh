#!/bin/sh
#
#  activate-household.sh
#  Stream TV Deployment
#
#  Created by ChrisRodStar.
#
#  Summary:
#  Activates the password-free Household Access release on the server. Backs up current
#  application state, hot-swaps the API binary and web assets, and restarts the
#  application container without interrupting catalog or streaming services.
#
set -eu

cd /opt/stream-tv/current
stage="/opt/stream-tv/staging/household-20261017"
backup="/opt/stream-tv/backups/pre-household-20261017"
umask 077

# Step 1: Validate payload checksums before making any changes
python3 - "$stage" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
checksums = json.loads((root / 'checksums.json').read_text())

for filename, expected_digest in checksums.items():
    file_bytes = (root / filename).read_bytes()
    actual_digest = hashlib.sha256(file_bytes).hexdigest()
    if actual_digest != expected_digest:
        raise SystemExit(f'Household release transfer checksum mismatch for {filename}')

print('Household API/web transfer checksums match verified.')
PY

# Step 2: Create pre-activation safety backup
mkdir "$backup"
readlink components/api/current > "$backup/api-version.txt"
cp -a artifacts/web "$backup/web"
cp source-versions.json "$backup/source-versions.json"

# Step 3: Stop the application container gracefully before updating state
docker compose stop --timeout 60 application

# Step 4: Retain database state and matching private encryption keys together
cp -a /opt/stream-tv/state/application "$backup/state"
cp -a /opt/stream-tv/private/application "$backup/private"

# Step 5: Perform atomic file activation and asset updates
python3 - "$stage" "$backup" <<'PY'
import json
from pathlib import Path
import shutil
import sys

stage_dir = Path(sys.argv[1])
backup_dir = Path(sys.argv[2])
release_dir = Path.cwd()

# Activate versioned API binary
version_info = json.loads((stage_dir / 'source-versions.json').read_text())
api_commit = (version_info.get('stv-api') or version_info['family-stream-api'])['commit']
api_destination = release_dir / 'components/api' / api_commit
api_destination.mkdir(mode=0o755, parents=True, exist_ok=True)

shutil.copy2(stage_dir / 'api', api_destination / 'api')
(api_destination / 'api').chmod(0o755)

next_link = release_dir / 'components/api/household-next'
next_link.unlink(missing_ok=True)
next_link.symlink_to(api_commit)
next_link.replace(release_dir / 'components/api/current')

# Update web assets while preserving directory inode and caching
shutil.copytree(stage_dir / 'web/assets', release_dir / 'artifacts/web/assets', dirs_exist_ok=True)
for source_file in (stage_dir / 'web').iterdir():
    if source_file.is_file():
        temp_dest = release_dir / 'artifacts/web' / f"{source_file.name}.next"
        shutil.copy2(source_file, temp_dest)
        temp_dest.replace(release_dir / 'artifacts/web' / source_file.name)

shutil.copy2(stage_dir / 'source-versions.json', release_dir / 'source-versions.json')
PY

# Step 6: Restart application container with updated binary and web assets
docker compose up -d --no-build application
printf 'Household access activated. Existing data and previous application artifacts retained.\n'
