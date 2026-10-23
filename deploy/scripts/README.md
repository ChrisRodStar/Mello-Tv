<div align="center">

# Deployment Scripts Reference

**Execution targets, working directories, and operational descriptions.**

[Overview](#overview) &bull; [Script Execution Matrix](#script-execution-matrix) &bull; [Usage Guidelines](#usage-guidelines)

---

</div>

## Overview

Scripts in this directory belong to two distinct operational environments:
1. **Mac Development Machine:** Scripts that compile binaries or prepare local data before transfer.
2. **Raspberry Pi Server (Server):** Scripts that manage containers, network routes, and atomic releases directly on the home server.

> [!IMPORTANT]
> Never execute Pi-targeted scripts directly on the Mac. Running a script in the wrong environment will cause path failures or broken configurations.

---

## Script Execution Matrix

| Script | Target Host | Working Directory | Plain English Purpose |
| :--- | :--- | :--- | :--- |
| `build-relay.sh` | **Mac mini** | `deploy/` | Compiles the Linux ARM64 relay binary via a Docker volume mount reading from `relay/`. |
| `migrate-mac.py` | **Mac mini** | `deploy/` | Quiesces local writers, takes a consistent backup of Mac databases/keys, and prepares a migration payload. |
| `cutover.sh` | **Pi (Server)** | `/opt/stream-tv/releases/20261017` | Stops legacy proof-of-concept containers and switches Tailscale Serve routes to the V2 stack. |
| `rollback.sh` | **Pi (Server)** | `/opt/stream-tv/releases/20261017` | Emergency rollback: stops V2 containers, restores legacy containers, and resets Tailscale Serve to port 3000. |
| `activate-household.sh` | **Pi (Server)** | `/opt/stream-tv/current` | Activates household access update, removing password gates while preserving personal profiles. |
| `rollback-household.sh` | **Pi (Server)** | `/opt/stream-tv/current` | Reverts household update to the previous API/web version. |
| `finish-routing.sh` | **Pi (Server)** | `/opt/stream-tv/current` | Helper to re-apply the three Tailscale Serve routes (`/`, `/api`, `/stream`). |
| `activate-components.py`| **Pi (Server)** | `/opt/stream-tv/releases/20261017` | Helper that atomically symlinks staged component binaries into production paths. |
| `finish-packaging.sh` | **Pi (Server)** | `/opt/stream-tv/releases/20261017` | Helper that extracts relay binary from builder, symlinks components, and builds images. |

---

## Usage Guidelines

### Running Mac Scripts
Execute directly from the `deploy/` folder on your Mac mini:
```bash
# Build the relay binary for ARM64 Linux
sh scripts/build-relay.sh

# Run the Mac state migration tool
python3 scripts/migrate-mac.py
```

### Running Pi Scripts
Execute on the server via SSH from your Mac terminal:
```bash
# Example: Triggering a rollback on the server
ssh user@server.local 'sh /opt/stream-tv/current/scripts/rollback.sh'
```
