<div align="center">

# Production Deployment Specification

**The master mission brief and requirements for deploying Stream TV to the server.**

[Mission Brief](#mission-brief) &bull; [Core Constraints](#core-deployment-constraints) &bull; [Migration Requirements](#data-migration-requirements) &bull; [Safety Policy](#safety-and-rollback-policy)

---

</div>

## Mission Brief

This specification document outlines the instructions for packaging and deploying Stream TV to the home Raspberry Pi (**Server**), replacing the earlier proof-of-concept, and migrating existing Mac watch history and profiles.

> [!IMPORTANT]
> The workspace at `.` organizes six separately maintained projects. Deployments must preserve project and container boundaries without creating a monolithic codebase.

---

## Core Deployment Constraints

For full architectural topology and network diagrams, see [deploy/README.md](../README.md).

1. **Three Containers Only:**
   - **`application`:** Go API and pre-compiled React/Vite web assets.
   - **`catalog`:** Go metadata and recommendation service with local SQLite storage.
   - **`streaming`:** Native Go provider scrapers, Rust media relay, and Mullvad VPN client.
2. **Fail-Closed VPN Defense:**
   - Provider searches and media streaming inside the `streaming` container must route strictly through the `wg0` WireGuard tunnel.
   - Outbound traffic must be immediately blocked by firewall rules if the VPN drops.
3. **Single Private Address:**
   - Tailscale Serve runs on the host as the single private ingress:
     - `/` routes to the web interface.
     - `/api/*` routes to the Go API.
     - `/stream/*` routes directly to the Rust relay.
   - The Pi is not a Tailscale exit node, and public Funnel access is disabled.
4. **Direct Artwork Loading:**
   - Movie and TV show posters are loaded and cached directly by client browsers/apps from TMDB's edge servers (`image.tmdb.org`).
   - Servers store lightweight URL references only, saving disk space.

---

## Data Migration Requirements

Instead of starting with a blank database, all existing Mac data must be migrated over safely:

- **What Gets Migrated:**
  - Active API SQLite database (accounts, profile IDs, watch history, continue-watching shelves, favorites).
  - Database encryption keys and Ed25519 signing keys.
  - Retained relay database and verification keys.
  - WireGuard credentials and upstream TMDB configuration.
- **Consistent Backup First:**
  - Stop active Mac writers before transfer to prevent database corruption.
  - Validate SHA-256 checksums across all transferred files before starting containers.
  - Keep secrets and keys out of Git, Docker images, and logs.

---

## Safety and Rollback Policy

1. **No Data Deletion:**
   - Never use `docker compose down -v` or prune storage volumes.
   - Keep the original `family_stream_data` volume preserved and untouched.
2. **Automatic Rollback Ready:**
   - Keep a working one-click rollback script (`scripts/rollback.sh`) that stops V2 and restores the legacy system immediately if needed.
3. **Manual Human Acceptance:**
   - AI agents must not run destructive automated test suites or fake simulated viewing.
   - All real-world testing (logging in, watching movies, testing seeking and audio, reboot persistence) is performed and confirmed directly by the operator using the [ACCEPTANCE.md](ACCEPTANCE.md) checklist.
