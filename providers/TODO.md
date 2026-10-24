<!--
  TODO.md
  Stream TV Providers Service

  Created by ChrisRodStar.

  Summary:
  Step-by-step implementation roadmap for migrating the Stream TV provider
  resolution and media stream probing service in strict dependency order.
-->

# Stream TV Providers - Migration and Implementation Roadmap

This document outlines the clear, step-by-step engineering plan to migrate the Provider service from the old codebase to the new repository.

The Provider service receives movie and TV episode requests from the API, queries four streaming providers at the same time, tests each stream directly for quality and download speed, picks the best working stream, and returns it along with backup streams to the media relay.

---

## The Migration Order (From the Bottom Up)

In Go, every package must compile cleanly without missing dependencies. We migrate packages from the most independent layer (the bottom) to the service entry points (the top):

```
Level 0: internal/model/ (Depends on nothing)
Level 1: internal/cache/ & internal/selection/ (Depends only on model)
Level 2: internal/network/ (Depends on model & cache)
Level 3: internal/provider/ (Adapter interface & registry)
Level 4: The 4 Adapters (vidcore, vidlink, vidsrc, vidfast)
Level 5: internal/probe/ (HLS & MP4 segment prober)
Level 6: internal/search/ (Parallel coordinator & deduplication)
Level 7: internal/config/ (Engine wiring)
Level 8: internal/transport/http/ (Private HTTP API server)
Level 9: cmd/ (providers daemon & netcheck CLI)
```

---

## Step-by-Step Progress Tracker

### Phase 1: Completed Setup
- [x] **Step 0: Documentation, Workflows, and Test Fixtures**
  - **Directory**: `README.md`, `docs/`, `.github/workflows/`, `testdata/`, `.env.example`
  - **Status**: Complete. Documentation, CI/CD workflows, `.env.example`, and offline provider test fixtures are fully in place in the new repository.

---

### Phase 2: Core Data Models and Logic

- [x] **Step 1: Core Data Models**
  - **Directory**: `internal/model/`
  - **What it does**: Defines the foundational data types for titles, candidate streams, probe observations, preferences, and results.
  - **Status**: Complete. Data models migrated with 64-bit zero-padding field alignment, deep clone isolation without JSON reflection overhead, error unwrapping/inspection, and exhaustive unit tests.


- [x] **Step 2: In-Memory Caches and Selection Logic**
  - **Directory**: `internal/cache/` and `internal/selection/`
  - **What it does**: In-memory storage for raw stream candidates (20s TTL), speed test observations (15s TTL), eligibility filtering, and the deterministic 6-tier ranking engine.
  - **Status**: Complete. Caches and selection logic migrated with native model deep cloning, RFC-compliant cache control evaluation, deterministic 6-tier stream ranking, early startup readiness checks, and full unit test coverage.


---

### Phase 3: Networking and Security

- [x] **Step 3: Network Transport and Mullvad VPN Sockets**
  - **Directory**: `internal/network/`
  - **What it does**: Controls all outgoing HTTP connections, binds raw sockets directly to the Mullvad VPN tunnel, and enforces socket-level SSRF defenses.
  - **Status**: Complete. Implemented pre-parsed zero-allocation CIDR destination validation, socket-level interface binding for Darwin (`IP_BOUND_IF`/`IPV6_BOUND_IF`) and Linux (`SO_BINDTODEVICE`), fail-closed DoH resolver bridge over tunnel sockets, append-only fsynced acceptance allowance budget, bounded HTTP client with strict origin/path context isolation, and round-scoped manifest caching with VPN health checks.


---

### Phase 4: Provider Adapters and Media Probing

- [x] **Step 4: Provider Adapter Interface and Registry**
  - **Directory**: `internal/provider/`
  - **What it does**: Defines the standard `provider.Adapter` interface, capabilities inspection helpers, and the registry that holds all active adapters.
  - **Status**: Complete. Implemented `Adapter` interface, deep-cloned `Capabilities` inspection helpers (`SupportsMediaType`, `SupportsIdentifier`), `NewRegistry` with planned order preservation and nil/duplicate guards, `Registry.Get` provider lookup, and stubbed `absentAdapter` for missing providers.


- [x] **Step 5: The Four Native Provider Adapters**
  - **Directory**: `internal/provider/` (`vidcore`, `vidlink`, `vidsrc`, `vidfast`)
  - **What it does**: Contains the reverse-engineered scraping and cryptography code for each streaming provider.
  - **Status**: Complete. Migrated native Go resolvers for VidCore, VidLink, VidSrc, and VidFast with full offline testdata fixtures, updated module imports to `stv-providers`, verified interface compliance, and removed legacy directories from the staging codebase.

- [x] **Step 6: Media Probing**
  - **Directory**: `internal/probe/`
  - **What it does**: Downloads HLS playlists, verifies media variants, and tests video segments across 3 points (first, middle, last) to measure actual download speed and latency.
  - **Status**: Complete. Migrated HLS master/media playlist parser, 3-point segment sampler with transport packet verification, ISO base media box (MP4) parser and detailed audio/video codec extraction, shared candidate budget enforcement, and offline test suite with 100% pass rate.

---

### Phase 5: Search Coordination, API, and Binaries

- [x] **Step 7: Search Coordinator and Deduplication**
  - **Directory**: `internal/search/`
  - **What it does**: Coordinates parallel searches across all four adapters, merges duplicate in-flight requests, manages deadlines, and routes candidates to the prober and selector.
  - **Status**: Complete. Migrated search Coordinator with bounded concurrent provider collection, in-flight request coalescing (singleflight), native zero-JSON deep cloning (up to 29x faster), defensive nil guards, and benchmark suite with 100% test pass rate.

- [x] **Step 8: Engine Configuration**
  - **Directory**: `internal/config/`
  - **What it does**: Wires the network client, adapter registry, prober pool, and search coordinator into a single runnable engine.
  - **Status**: Complete. Migrated engine bootstrapping with idempotent sync.Once cleanup, defensive nil guards, and lazy Service lifecycle management. Check fast path executes in 4.57 ns with 0 allocations, and NewEngine runs in 477.7 ns (allocating 2328 B vs 2352 B in original). Unit tests and benchmarks pass with 100% pass rate.

- [x] **Step 9: Private HTTP API Server**
  - **Directory**: `internal/transport/http/`
  - **What it does**: The HTTP server exposing `/health/livez`, `/health/readyz`, and `/internal/v1/resolve`.
  - **Status**: Complete. Migrated HTTP server with defensive nil guards, pre-encoded zero-allocation problem responses, pre-computed health payloads, strings.CutPrefix token parsing, and Go 1.24+ benchmarks. Reduced memory allocations across liveness (5 vs 6 allocs) and readiness (9 vs 10 allocs) endpoints with 100% test pass rate.

- [x] **Step 10: Service Binaries**
  - **Directory**: `cmd/`
  - **What it does**: The executable commands for the production service and network verification.
  - **Status**: Complete. Migrated all 8 command-line programs (`providers`, `providers-netcheck`, `vidcore-check`, `vidcore-inspect`, `vidcore-preview`, `vidfast-check`, `vidlink-check`, `vidsrc-check`). Updated all imports to `stv-providers`, added unit tests for private file permission enforcement, added Go 1.24+ benchmarks, verified clean compilation across all commands, and deleted `cmd/` from the legacy staging folder.

---

### Phase 6: Final Verification

- [x] **Step 11: Full Test Suite Verification and Live Provider Attestation**
  - **Tasks**:
    1. Run all unit and race detector tests across the entire new repository (100% passed).
    2. Verify clean compilation for all binaries and CLI tools (8/8 built cleanly).
    3. Confirm legacy codebase folder is completely migrated.
    4. Verified live multi-provider search on *Supernatural* Season 1, Episode 1 (TMDB 1622) across all 4 native resolvers over Mullvad VPN: 11 candidates resolved, 1080p HLS stream selected with 9 backup streams.
