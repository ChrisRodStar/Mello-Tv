"""Exercise the built Docker image using isolated, disposable application state."""

import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import urllib.request
import uuid


def main():
    root = Path(__file__).resolve().parent.parent
    project = "mello-smoke-" + uuid.uuid4().hex[:12]
    environment = dict(os.environ)
    environment.update(
        MELLO_PORT=os.environ.get("MELLO_SMOKE_PORT", "18080"),
        MELLO_BIND_ADDRESS="127.0.0.1",
        TMDB_API_KEY="",
        TMDB_READ_ACCESS_TOKEN="",
    )
    base = "http://127.0.0.1:" + environment["MELLO_PORT"]

    with tempfile.TemporaryDirectory(prefix="mello-smoke-") as temporary:
        empty_env = Path(temporary) / "empty.env"
        empty_env.touch()
        command = ["docker", "compose", "--env-file", str(empty_env), "-p", project]

        def compose(*args, capture=False):
            return subprocess.run(
                command + list(args), cwd=root, env=environment, check=True,
                stdout=subprocess.PIPE if capture else None, timeout=180,
            ).stdout

        def request(path, payload=None):
            body = None if payload is None else json.dumps(payload).encode()
            req = urllib.request.Request(
                base + path, data=body,
                headers={"Content-Type": "application/json"} if body else {},
            )
            with urllib.request.urlopen(req, timeout=15) as response:
                content = response.read()
                if response.headers.get_content_type() == "application/json":
                    return json.loads(content)
                return content

        def key_digest():
            key = compose("exec", "-T", "mello", "cat", "/data/api-encryption", capture=True)
            assert len(key) == 32, "Encryption key must be 32 bytes"
            return hashlib.sha256(key).digest()

        try:
            config = json.loads(compose("config", "--format", "json", capture=True))
            image = config["services"]["mello"]["image"]
            for setup, expected_error in [
                ("touch /data/accounts.db", "Restore its original key"),
                ("printf invalid > /data/api-encryption", "exactly 32 bytes"),
            ]:
                result = subprocess.run(
                    ["docker", "run", "--rm", "--tmpfs", "/data:uid=10001,gid=10001",
                     "--entrypoint", "/bin/sh", image, "-c",
                     setup + "; exec /usr/local/bin/mello-entrypoint true"],
                    env=environment, capture_output=True, timeout=30,
                )
                assert result.returncode != 0, "Unsafe key state was accepted"
                assert expected_error in result.stderr.decode(), "Unexpected startup failure"
            compose("up", "-d", "--no-build", "--wait", "--wait-timeout", "90")
            assert request("/health/readyz")["status"] == "ready"
            html = request("/")
            assert b'<div id="root"' in html, "Missing web client"
            assets = re.findall(rb'(?:src|href)="(/assets/[^\"]+)"', html)
            assert assets, "Missing bundled assets"
            for asset in assets:
                content = request(asset.decode())
                assert content and content != html, "Missing bundled asset"
            assert request("/settings") == html, "SPA fallback failed"
            assert compose("exec", "-T", "mello", "id", "-u", capture=True).strip() == b"10001"
            compose("exec", "-T", "mello", "sh", "-c", "touch /tmp/mello-smoke && rm /tmp/mello-smoke")
            write = subprocess.run(
                command + ["exec", "-T", "--user", "0", "mello", "touch", "/app/mello-smoke"],
                cwd=root, env=environment, capture_output=True, timeout=15,
            )
            assert write.returncode != 0 and b"Read-only file system" in write.stderr, "Root filesystem is writable"
            profile = request("/api/v1/profiles", {"name": "Docker test", "color": "#446e88"})
            profile_id = profile["id"]
            original_key = key_digest()
            compose("down")
            compose("up", "-d", "--no-build", "--wait", "--wait-timeout", "90")
            saved = request("/api/v1/profiles/" + profile_id)
            assert saved["name"] == "Docker test", "Profile did not survive recreation"
            assert key_digest() == original_key, "Encryption key changed"
            print("PASS: health, web/assets, SPA, non-root/read-only runtime, profile/key persistence, key guards")
        finally:
            # The random project owns only this test's disposable volume.
            compose("down", "--volumes")


if __name__ == "__main__":
    main()
