#!/usr/bin/env python3
"""Compare every registry package in Cargo.lock with crates.io stable releases.

Freshness audit only, not a vulnerability or supply-chain security audit.
Requires Python 3.9+ and Cargo; does not install packages or change the lockfile.
"""
import concurrent.futures
import json
from pathlib import Path
import re
import subprocess
import sys
import urllib.request
from datetime import datetime, timezone

root = Path(__file__).resolve().parents[1]
manifest = json.loads(subprocess.check_output(
    ['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'], cwd=root))
direct = {dependency['name'] for package in manifest['packages']
          for dependency in package['dependencies']}
locked = {}
# Cargo's generated lockfile uses one [[package]] block per package. Read only
# its top-level quoted name/version/source fields, never dependency strings.
for block in (root / 'Cargo.lock').read_text().split('[[package]]')[1:]:
    fields = dict(re.findall(r'^(name|version|source) = "([^"\n]+)"$', block, re.M))
    if fields.get('source', '').startswith('registry+'):
        locked.setdefault(fields['name'], []).append(fields['version'])


def audit(name):
    url = 'https://crates.io/api/v1/crates/' + name
    request = urllib.request.Request(url, headers={
        'User-Agent': 'family-stream-relay-dependency-freshness-audit/1.0'})
    with urllib.request.urlopen(request, timeout=30) as response:
        data = json.load(response)
    latest = data['crate']['max_stable_version']
    if not latest:
        raise RuntimeError('No stable release reported for ' + name)
    latest_record = next(item for item in data['versions'] if item['num'] == latest)
    if latest_record['yanked']:
        raise RuntimeError('Registry reports a yanked latest release for ' + name)
    versions = sorted(locked[name])
    return {'name': name, 'direct': name in direct, 'locked_versions': versions,
            'latest_stable': latest, 'all_locked_latest': all(v == latest for v in versions),
            'source': url}


with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
    packages = sorted(pool.map(audit, sorted(locked)), key=lambda item: item['name'])
report = {'checked_at_utc': datetime.now(timezone.utc).isoformat(),
          'policy': 'latest non-yanked stable releases; includes all lockfile targets',
          'packages': packages}
if len(sys.argv) > 1:
    Path(sys.argv[1]).write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({'packages_checked': len(packages),
                  'direct_dependencies_checked': sum(p['direct'] for p in packages),
                  'direct_dependencies_latest': sum(p['direct'] and p['all_locked_latest'] for p in packages),
                  'behind_latest': [p for p in packages if not p['all_locked_latest']]}, indent=2))
