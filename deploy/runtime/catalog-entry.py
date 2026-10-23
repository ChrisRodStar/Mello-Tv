#!/usr/bin/python3
#
#  catalog-entry.py
#  Stream TV Deployment
#
#  Created by ChrisRodStar.
#
#  Summary:
#  Container entrypoint for the catalog service. Reads secrets from /run/secrets/catalog.json,
#  populates runtime environment variables, and launches the Go catalog binary.
#
import json
import os
from pathlib import Path

# Load catalog secrets and database configuration
config_path = Path("/run/secrets/catalog.json")
config = json.loads(config_path.read_text())

# Populate runtime environment variables for the Go service
os.environ.update(config)

# Replace the current process with the catalog binary
catalog_binary = "/opt/releases/catalog/current/catalog"
os.execv(catalog_binary, ["catalog"])
