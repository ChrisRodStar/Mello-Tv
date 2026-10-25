# Changelog

All notable changes to the Stream TV backend and media distribution architecture are documented in this file.

## [2.0.0] - 2026-10-25

### Complete Rust Rewrite & Architecture Modernization

This release replaces the legacy multi-service Go stack and standalone relay with a unified, high-performance, memory-safe **Rust workspace** (`crates/`), achieving dramatic resolution latency improvements, eliminating ambient dependencies, and consolidating all backend capabilities into a cohesive architecture.

---

### Key Architectural Transformations

1. **Unified Workspace Architecture**:
   - Replaced fragmented Go services (`api/`, `catalog/`, `providers/`, `relay/`) with 9 purpose-built Rust crates:
     - [`stream-core`](crates/core): Canonical domain types, media coordinates, quality preferences, and contract schemas.
     - [`stream-storage`](crates/storage): SQLite-backed persistence for accounts, profiles, watch history, shelves, and catalog metadata with WAL mode.
     - [`stream-provider-sdk`](crates/provider-sdk): Zero-ambient-authority provider interfaces and WIT definition bindings.
     - [`stream-provider-codecs`](crates/provider-codecs): Pure Rust and WebAssembly cryptographic ciphers (AES-CBC, AES-CTR, RC4, custom permutation ciphers).
     - [`stream-provider-runtime`](crates/provider-runtime): Bounded WASI/wasmtime component execution engine.
     - [`stream-provider-adapters`](crates/provider-adapters): Native scraper adapters for VidLink, Videasy, VidCore, VidSrc, and VidFast.
     - [`stream-media`](crates/media): Fail-closed media relay, adaptive bitrate HLS playlist rewriting, prober, and transport pooling.
     - [`stream-server`](crates/server): Axum HTTP API gateway, session lifecycle coordinator, and failover router.
     - [`stream-update`](crates/update): Atomic background update coordinator for catalog synchronization.

2. **Zero Headless Browsers / Zero Ambient Execution**:
   - 100% of provider token generation, decryption ciphers, and challenge workflows run in pure native Rust and sandboxed WebAssembly.
   - Eliminated reliance on Node.js runtimes, Puppeteer, or heavyweight browser engines.

---

### Provider Latency & Performance Optimizations

#### Comprehensive Benchmark Across Real Media Targets

| Provider | Inception *(Modern Sci-Fi)* | Fight Club *(Classic Drama)* | Breaking Bad S01E01 *(Prestige TV)* | Supernatural S01E10 *(Dark Fantasy TV)* | Attack on Titan S01E01 *(Anime)* | Streams Resolved |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: |
| **`vidlink`** | **673 ms** | **559 ms** | **658 ms** | **508 ms** | **816 ms** | **3–4 streams** *(Direct MP4)* |
| **`videasy`** | **1,190 ms** *(was 10,502 ms)* | **515 ms** | **1,198 ms** *(was 2,216 ms)* | **664 ms** *(was 2,202 ms)* | **1,209 ms** *(was 2,218 ms)* | **7–19 streams** *(Multi-bitrate HLS)* |
| **`vidcore`** | **1,972 ms** *(was 5,483 ms)* | **2,148 ms** *(was 4,121 ms)* | **1,732 ms** *(was 2,992 ms)* | **1,672 ms** *(was 3,800 ms)* | **2,722 ms** *(was 3,900 ms)* | **2–3 streams** *(Master HLS)* |
| **`vidsrc`** | **2,143 ms** | **1,716 ms** | **1,614 ms** | **1,677 ms** | **1,814 ms** | **3 streams** *(Wasm Decrypted HLS)* |
| **`vidfast`** | **3,372 ms** *(was 4,566 ms)* | **1,611 ms** *(was 4,346 ms)* | *(Not indexed upstream)* | *(Not indexed upstream)* | *(Not indexed upstream)* | **4–5 streams** *(HLS)* |

#### Engineering Optimizations Applied:
- **VidCore In-Memory Protocol Caching**: Cached compiled challenge/action protocols process-wide, eliminating redundant 2.5MB player chunk downloads on repeat lookups.
- **VidCore Parallel Master-Playlist Candidate Probes**: Replaced sequential HTTP GET probes with concurrent `batch()` probing (800ms cutoff) for `/sd/` master-rendition upgrades.
- **VidCore Server Unlock Clamping**: Capped fanout to the top 8 healthy mirrors (`Supreme`, `Prime`, `Horizon`, `Orbit`) with 1,000ms timeouts, eliminating dead-mirror latency penalties.
- **Videasy Mirror Timeout Clamping**: Reduced mirror request cutoff to 1,000ms; working mirrors (`boise`, `miami`, `phoenix`) resolve in 300–600ms without stalling behind dead mirrors.
- **VidFast Material Caching**: Added in-memory `ProtocolMaterial` caching keyed by player script URL, avoiding repeat 3MB JavaScript script downloads.
- **VidSrc Coordinate Normalization**: Fixed upstream TV episode coordinate deserialization to accept both string (`"1"`) and numeric (`1`) fields, restoring 100% resolution success across all episodic TV and anime.
- **Global In-Memory DNS Caching**: Added an in-memory 120-second TTL DNS resolver cache, cutting repeat socket resolution latency to 0 ms.
- **Connection Pre-Warming & Pool Tuning**: Primed TCP/TLS connection pools and enabled `TCP_NODELAY` and HTTP/2 multiplexing across all upstream CDN boundaries.

---

### Removed Legacy Projects

To eliminate code duplication and maintain a single source of truth, the following legacy directories have been removed:
- `api/` (Former Go API gateway)
- `catalog/` (Former Go SQLite catalog service)
- `providers/` (Former Go provider scrapers)
- `relay/` (Former standalone Rust relay)

### Preserved Components
- `web/` (React + Vite web client)
- `ios/` (SwiftUI mobile client)
- `deploy/` (Docker Compose deployment topologies)
- External reference directory `/Users/dev/Desktop/family-stream` preserved unchanged.

---

### Verification & Quality Assurance

- **Zero Clippy Warnings**: Strict `#![forbid(unsafe_code)]` compliance under `-D warnings` across all workspace crates.
- **Complete Test Suite**: All 100+ unit, contract, and integration tests passed (`cargo test --workspace`).
- **Live Verification**: End-to-end resolution harness verified with real upstream hosters across sci-fi, classic drama, prestige TV, dark fantasy, and anime.
