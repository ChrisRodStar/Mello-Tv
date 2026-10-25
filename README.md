<div align="center">

# Stream TV

**A private, resilient, fail-closed household media architecture.**

[Architecture](#system-architecture) &bull; [Projects](#the-project-fleet) &bull; [Privacy & Security](#privacy-and-fail-closed-routing) &bull; [Data Resilience](#data-and-resilience) &bull; [Development](#development-and-implementation-order)

---

</div>

## Overview

The backend has been completely migrated to a unified, high-performance, portable **Rust workspace** (`crates/`), replacing the legacy multi-service Go and standalone relay implementations with a single memory-safe binary suite:

- **100% Native Codecs & Ciphers**: Zero headless browsers, zero node runtimes. Provider ciphers (VidCore, VidLink, VidSrc, VidFast, Videasy) run directly in Rust WebAssembly and native crypto crates.
- **Sub-Second to Low-Latency Stream Resolution**: Integrated DNS caching, HTTP/2 connection pooling, parallel playlist promotion, and in-memory protocol compilation.
- **Fail-Closed Privacy & Relay**: Streaming proxy with HLS rewriting, bounded backpressure buffers, and robust token authorization.
- **Clients**: Native iOS (SwiftUI) and modern Web (React + Vite) frontends.

---

## System Architecture

The repository is organized into a modular Rust workspace and client applications:

<div align="center">

```mermaid
flowchart TD
    Client["Client Device (iPhone / Web Browser)"]

    subgraph Clients ["Client Applications"]
        Web["stream-web (React + Vite)"]
        IOS["stream-ios (SwiftUI + AVKit)"]
    end

    subgraph RustWorkspace ["Unified Rust Backend (crates/)"]
        Server["stream-server (API Gateway & Sessions)"]
        Storage["stream-storage (Catalog, Profiles, SQLite)"]
        Media["stream-media (Streaming Relay & Prober)"]
        Adapters["stream-provider-adapters (VidCore, VidLink, etc.)"]
        Runtime["stream-provider-runtime & codecs (Wasm Sandbox)"]
        Core["stream-core (Domain Types & Contracts)"]

        Server --> Storage
        Server --> Media
        Media --> Adapters
        Adapters --> Runtime
        Runtime --> Core
    end

    Internet["Upstream Video Hosters & CDNs"]

    Client --> Web
    Client --> IOS
    Web -->|"API & Stream RPC"| Server
    IOS -->|"API & Stream RPC"| Server
    Media -->|"Protected Stream Ingestion"| Internet
```

</div>

---

## The Project Fleet

| Directory | Stack | Responsibility |
|---|---|---|
| [crates/](crates) | Rust 2024 Workspace | **The Unified Backend:** High-performance server, catalog database, media streaming relay, and reverse-engineered provider cipher engine. |
| [deploy/](deploy) | Docker Compose | **The Orchestrator:** Container and service deployment configurations. |
| [ios/](ios) | SwiftUI + AVKit | **The iOS Client:** Native mobile interface with speculative background player pre-warming. |
| [web/](web) | React + Vite | **The Web Client:** Clean browser interface featuring keyboard navigation, tap-to-seek gestures, and continue-watching shelves. |

---

## Privacy and Fail-Closed Routing

Stream TV enforces absolute network isolation so that client devices never communicate directly with upstream video hosts.

<div align="center">

```mermaid
flowchart TD
    Device["Client Device (iPhone / Browser)"] -->|"1. Tailscale Encrypted Mesh"| Host["Tailscale Serve on Host"]
    
    Host -->|"Path /"| Web["Application Container: Web UI"]
    Host -->|"Path /api/*"| API["Application Container: Go API"]
    Host -->|"Path /stream/*"| Relay["Streaming Container: Rust Relay"]

    subgraph VPNBoundary ["Streaming Container (Isolated Network Namespace)"]
        Relay -->|"2. Fail-Closed iptables"| WireGuard["Mullvad VPN (wg0)"]
    end

    WireGuard -->|"3. Protected Outbound Traffic"| Hosters["Upstream Video Hosters"]
```

</div>

1. **Private Ingress:** Tailscale Serve acts as the single entry point. Remote devices connect over a private, authenticated mesh tunnel.
2. **Fail-Closed VPN Egress:** All provider scraping and media fetching in the streaming container must route through the Mullvad WireGuard interface (`wg0`). If the tunnel disconnects, outbound traffic drops immediately via kernel firewall rules to prevent IP exposure.
3. **Manifest Link Sanitization:** The Rust relay parses all `.m3u8` playlists and rewrites external segment URLs into local relay paths (`/stream/{session_id}/r/{resource_id}?st={token}`). Client devices never receive direct addresses to third-party CDNs.

---

## Data and Resilience

The system is designed to survive upstream outages without breaking the home viewing experience:

- **Offline-First Metadata:** The catalog service continuously pulls updates from TMDB, but the API maintains a cached read snapshot. If TMDB has an outage, family members can still browse shelves and watch saved titles.
- **Direct Artwork Delivery:** Browsers and apps load poster and backdrop images directly from TMDB's edge CDN. The local database stores lightweight URL references only, avoiding server disk consumption.
- **Self-Contained Resolution:** Playback requests include canonical TMDB IDs along with IMDb/MyAnimeList cross-IDs and TV coordinates. Providers resolve media without waiting on a synchronous catalog lookup.
- **Ordered Fallback Streams:** When an episode begins, the API pre-stages ranked backup sources. If an upstream hoster stutters or drops, the player can trigger an instant failover.

---

## Development and Implementation Order

Development and validation follow a strict dependency order:

<div align="center">

```mermaid
flowchart TD
    Step1["1. Contracts & Identifiers<br/>Define canonical TMDB/IMDb IDs & JSON schemas"]
    Step2["2. Catalog Persistence & Snapshot<br/>Implement SQLite storage & background sweeps"]
    Step3["3. Native Provider Resolution<br/>Pure-Go cipher solvers & stream probing"]
    Step4["4. Media Relay Delivery<br/>Rust HLS rewriting & bounded streaming"]
    Step5["5. Public API Gateway<br/>Client auth, session control & failover"]
    Step6["6. Client Integration<br/>Connect iOS and Web clients to public API"]

    Step1 --> Step2 --> Step3 --> Step4 --> Step5 --> Step6
```

</div>

### Working Conventions

- All active development can run locally. Production deployment targets Docker Compose with optional Tailscale Serve routing.
- Secrets, WireGuard private keys, and local SQLite databases must never be committed to source control.
