#!/usr/bin/env python3
"""Run Go encoding/json against a disposable copy of the real provider models.
Does not write to the provider checkout, resolve modules, or contact providers.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
providers = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else root.parent / 'family-stream-providers'
out = root / 'tests/fixtures/providers'
manifest = {}
with tempfile.TemporaryDirectory(prefix='relay-go-goldens-') as temporary:
    work = Path(temporary)
    (work / 'internal/model').mkdir(parents=True)
    for name in ('title.go', 'source.go', 'result.go'):
        source = providers / 'internal/model' / name
        manifest[name] = hashlib.sha256(source.read_bytes()).hexdigest()
        shutil.copyfile(source, work / 'internal/model' / name)
    (work / 'go.mod').write_text('module family-stream-providers\n\ngo 1.26.0\n')
    shutil.copyfile(root / 'tools/golden.go', work / 'main.go')
    env = dict(os.environ, GOTOOLCHAIN='local', GOPROXY='off', GOSUMDB='off')
    subprocess.run(['go', 'run', '.', str(out)], cwd=work, env=env, check=True)
(out / 'provenance.json').write_text(json.dumps(manifest, indent=2) + '\n')
