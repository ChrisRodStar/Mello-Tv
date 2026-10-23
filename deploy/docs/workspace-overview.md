<div align="center">

# Stream TV Workspace Reference

**High-signal architecture summary, component boundaries, and workspace rules.**

[Overview](#overview) &bull; [Architecture & Boundaries](#architecture-and-boundaries) &bull; [Resilience & Privacy](#resilience-and-privacy) &bull; [Workspace Policies](#workspace-policies)

---

</div>

## Overview

Stream TV is a private, resilient home streaming ecosystem. It replaces commercial streaming bloat and ad-heavy public streaming sites with an automated home server running on a Raspberry Pi (**Server**) that discovers, solves, and streams high-definition video directly to personal iPhones, iPads, and browsers.

> [!NOTE]
> - For full project breakdowns, see the root [README.md](../../README.md#the-project-fleet).
> - For Docker container topology and network diagrams, see [deploy/README.md](../README.md#the-three-containers).

---

## Architecture and Boundaries

### 1. Six Independent Projects
This workspace orchestrates six separately maintained Git projects with simple local folder names and matching `stv-*` GitHub repositories. It is **not** an application monorepo:

- [api/](../../api) (`stv-api`): Go API gateway, profiles, watch history, and Ed25519-signed playback tokens.
- [catalog/](../../catalog) (`stv-catalog`): Go catalog service, SQLite storage, TMDB background synchronization.
- [providers/](../../providers) (`stv-providers`): Pure-Go scrapers resolving streams across VidCore, VidLink, VidSrc, and VidFast.
- [relay/](../../relay) (`stv-relay`): Rust/Tokio media engine, HLS manifest rewriting, and bounded streaming.
- [ios/](../../ios) (`stv-ios`): Native SwiftUI/AVKit app with speculative stream pre-warming.
- [web/](../../web) (`stv-web`): React/Vite browser client with continue-watching shelves.

### 2. Three Container Topology
The six projects deploy on the server as **exactly three synchronized containers**:
1. **`application` (Port 8090):** Go API serving its routes alongside the built React/Vite web assets.
2. **`catalog`:** Go metadata service owning its private SQLite database.
3. **`streaming` (Port 8095):** Go providers and Rust relay supervised under `supervisord` inside a locked Mullvad VPN network namespace.

---

## Resilience and Privacy

- **Fail-Closed VPN Egress:** All provider scraping and media streaming in the `streaming` container are bound strictly to `wg0`. If the VPN drops, the kernel firewall immediately blocks all outbound packets to prevent IP leakage.
- **Offline-First Catalog:** The API holds a cached read snapshot of the catalog. If upstream metadata providers (TMDB) suffer an outage, browsing saved shelves and watching existing titles remain fully functional.
- **Edge Artwork Delivery:** Client browsers and devices load poster/backdrop images directly from TMDB's edge servers (`image.tmdb.org`). The server stores lightweight image URLs only, avoiding local disk usage.
- **Independent Failure Domains:** Provider updates do not restart the relay or VPN. Catalog updates do not restart the API or streaming containers.

---

## Workspace Policies

| Policy | Rule |
| :--- | :--- |
| **No Monorepo Merging** | Maintain independent release tags, Git histories, and private contracts across all six repositories. |
| **Command Restrictions** | Do not execute tests, builds, benchmarks, or deployment commands without an explicit user request. |
| **Credential Safety** | Never commit, print, copy, or log private keys, WireGuard tokens, or sudo passwords. |
| **Preserve Deployments** | Never run `docker compose down -v` or prune persistent volumes on the server. Keep rollback backups before any cutover. |
| **Local-First Testing** | All initial development runs locally on the Mac mini. Remote work on the server occurs only when explicitly authorized. |
