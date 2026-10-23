#!/usr/bin/env python3
#
#  activate-components.py
#  Stream TV Deployment
#
#  Created by ChrisRodStar.
#
#  Summary:
#  Stages immutable component binaries and atomically activates versions using
#  symlinks based on the pinned commits in source-versions.json.
#
import json
from pathlib import Path
import shutil

# Root directory of the deployment repo
ROOT_DIR = Path(__file__).resolve().parents[1]
SOURCE_VERSIONS_PATH = ROOT_DIR / "source-versions.json"
COMPONENTS_DIR = ROOT_DIR / "components"
ARTIFACTS_DIR = ROOT_DIR / "artifacts"

# Component definitions: (component_name, repository_name, binary_file_name)
MANAGED_COMPONENTS = [
    ("api", "stv-api", "api"),
    ("catalog", "stv-catalog", "catalog"),
    ("providers", "stv-providers", "providers"),
    ("relay", "stv-relay", "stv-relay"),
]


def activate_component(component_name: str, project_name: str, binary_name: str, commit_sha: str):
    """Copies a component binary to its versioned directory and points the 'current' symlink to it."""
    component_dir = COMPONENTS_DIR / component_name
    version_dir = component_dir / commit_sha
    version_dir.mkdir(parents=True, exist_ok=True)

    # Copy binary into versioned destination and ensure executable permissions
    source_binary = ARTIFACTS_DIR / binary_name
    dest_binary = version_dir / binary_name
    shutil.copy2(source_binary, dest_binary)
    dest_binary.chmod(0o755)

    # Perform atomic symlink replacement via a temporary link
    temp_link = component_dir / "next"
    temp_link.unlink(missing_ok=True)
    temp_link.symlink_to(commit_sha)
    temp_link.replace(component_dir / "current")

    print(f"Activated {component_name} -> {commit_sha[:7]}")


def main():
    if not SOURCE_VERSIONS_PATH.exists():
        raise FileNotFoundError(f"Missing required versions file: {SOURCE_VERSIONS_PATH}")

    versions = json.loads(SOURCE_VERSIONS_PATH.read_text())

    for component, project, binary in MANAGED_COMPONENTS:
        commit_sha = versions[project]["commit"]
        activate_component(component, project, binary, commit_sha)


if __name__ == "__main__":
    main()
