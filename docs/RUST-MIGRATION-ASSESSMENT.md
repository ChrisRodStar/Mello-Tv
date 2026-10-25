# Stream TV Rust migration assessment

Date: 2026-10-25. Status: discovery and proposed architecture; implementation has not started in this assessment.

## Product direction

Build a portable headless Stream TV backend for native clients, primarily iOS. Ship a Rust application for macOS, Linux, and Windows. Use ordinary network transports with explicit HTTP/SOCKS5 proxy configuration. Provider implementations must be independently installable and updatable. Application updates must have their own supported release lifecycle.

These requirements supersede the household-only, VPN-bound, three-container architecture described in existing repository documentation for the new distribution. They do not authorize changing an existing deployment during migration. Web source is outside the target product scope and need not be deleted.

## Current implementation and evidence

The committed application is Go API + Go catalog + Go providers + Rust relay, with iOS and web clients. Root Cargo.toml, Cargo.lock and crates/ were already untracked at the start of this assessment. Preserve this existing work while replacing its incomplete contracts deliberately.

| Area | Actual state | Migration consequence |
| --- | --- | --- |
| API | Accounts/state tables, household access, profiles, lists, ordered progress mutations, catalog snapshots, recommendations, playback authorization/renewal/recovery | Preserve externally visible contracts and persisted identities; a few browse/profile handlers are insufficient |
| Catalog | SQLite records, jobs, revisions, upstream refresh, stale reads, recommendation generation | Preserve data and refresh semantics; search is only one part |
| Providers | Five native implementations: VidCore, VidLink, VidSrc, VidFast, Videasy | Include all five; older documentation says four |
| Resolution | Concurrent discovery and probing, scoped request coalescing, caches, byte budgets, progressive startup, complete backups, device/language eligibility | Preserve existing cancellation and admission behavior, and distinguish early startup from final completion |
| Ranking | Language, established delivery, resolution, adaptive ladder, score, TTFB, ID ordering | Actual source has an adaptive criterion absent from the pasted six-tier description |
| Relay | Signed grants, encrypted session persistence, source generations, HLS rewriting, ranges, bounded streaming and recovery | Adapt and integrate existing relay rather than replacing it with the empty Rust MediaRelay struct |
| Rust draft | Basic domain structs, empty provider/prober/relay types, no-op migrations/repositories, empty routes, main logs and exits | Scaffold only; no functional migration acceptance |
| Deployment | Linux/VPN Compose and component activation scripts | Not a portable application installer/updater; documented automation/release.py is absent |
| CI | Ubuntu Go, relay, and web jobs | Add root workspace checks and native macOS/Linux/Windows execution |

Primary source locations:

- API lifecycle and contracts: api/internal/api/{http,playback,playback_backups,store,progress,household,model}.go.
- Catalog persistence/refresh: catalog/internal/{db/store,service/service,upstream/upstream}.go.
- Client contract: ios/FamilyStream/Core/API/{FamilyStreamAPI,APIModels}.swift.
- Provider contract: providers/internal/model/, internal/provider/{adapter,registry}.go, internal/config/config.go.
- Resolution: providers/internal/search/coordinator.go; internal/probe/; internal/selection/{eligibility,ranking,startup,fallback}.go.
- Relay: relay/src/{auth,control,session,hls,media,network,runtime}; relay/tests/.

## Verified baseline

- Installed compiler: rustc 1.99.0; Go 1.27.2 on macOS arm64.
- Official Rust release information identifies 1.99.1 (2026-10-15) as the current stable patch. Use a pinned reviewed stable toolchain when implementing; this assessment did not modify installed tools.
- `cargo check --workspace --all-targets`: FAIL, unresolved ProfileId/TitleId imports in crates/storage/src/repository/mod.rs.
- `cargo metadata --manifest-path relay/Cargo.toml --no-deps --format-version 1`: FAIL, relay believes it belongs to root workspace but is neither a member nor excluded.
- `cargo test -p stream-core`: PASS, zero unit tests and zero doctests.
- `cargo test -p stream-media`: PASS, zero unit tests and zero doctests.
- `go test ./...` in api and catalog: PASS; some packages used cached results.
- `go test ./...` in providers: PASS, including cached Videasy package. That suite contains live-network tests; this is not fresh provider availability evidence.
- A separate `go test -short -count=1 ./...` in providers passed to establish a fresh offline baseline. Keep live integration tests explicitly opt-in in the Rust design.
- No Windows/Linux execution, current relay test run, deployment, device acceptance, or measured speed improvement was established.

## Proposed structure

Keep one Rust workspace and a single default daemon process. Use crate boundaries for ownership and testability; independently updatable providers require a runtime package boundary as well.

| Boundary | Responsibility |
| --- | --- |
| core | Validated identifiers, title coordinates, candidates, observations, preferences, errors and contract DTOs; no application-crate dependencies or I/O |
| storage | SQLite repositories, migrations, user/playback state and bounded TTL caches |
| provider SDK / interface | Versioned component contract and host capabilities, independent of storage and server internals |
| provider runtime | Module installation/selection, component execution, cancellation, quotas and version lifecycle |
| media | Shared transport, destination policy, probing, eligibility, ranking and resolution coordination |
| existing relay | Native authenticated media delivery and durable relay sessions, integrated through a library entrypoint |
| application services | Catalog refresh/recommendations and playback orchestration; may initially be internal server modules |
| update | Signed metadata verification, artifact staging, compatibility and activation state |
| server | Configuration, API DTO mapping, authentication, CLI, task supervision and graceful shutdown |

Core means zero dependencies on other application crates, not zero third-party dependencies: serde and thiserror are compatible with that goal. Do not force media to depend on storage where a narrow cache interface suffices. TMDB network access belongs in application services rather than the persistence crate. Do not force all errors into a shared string-only error enum: preserve typed causes and redact errors at the public API boundary.

Evaluate rusqlite with bundled SQLite and a bounded blocking worker because the existing relay already uses it; keep SQLx only if a concrete benefit justifies two persistence stacks. SQLite calls must not block Tokio workers. Preserve both existing database schemas and their encryption/key relationships before considering consolidation. Back up through service-owned consistent snapshots, never copies of a live database file.

## Independently updatable providers

Recommended first architecture to validate: Rust-written WebAssembly components loaded by Wasmtime, using a versioned WIT interface. Native worker processes with a versioned IPC protocol are a fallback if component compatibility or measured overhead proves unacceptable. Workers require platform-specific artifacts and process lifecycle handling; processes alone do not provide a sandbox.

An ordinary Rust module or crate still ships inside the daemon and cannot satisfy independent runtime updates. Native Rust dynamic libraries do not have a stable Rust ABI. A component interface avoids requiring the user's daemon and provider to share a Rust compiler build.

Each provider package should declare:

- Stable provider identifier and independent package version.
- Interface version, supported host versions and supported content/identifier capabilities.
- Signed artifact identity, digest and length, governed by authenticated update metadata.
- Explicit requested network capabilities, constrained further by host policy.
- Resource limits and a package test/vector set.

The host owns HTTP/TLS/DNS, proxy selection, redirect rules, address validation, connection reuse, cookie/header scope, response size and deadline limits. Components receive bounded responses through host functions. Do not grant ambient sockets, arbitrary filesystem access, environment secrets or process execution. A signed provider must still have bounded privileges.

VidCore currently recognizes known material/decoded-bytecode fingerprints and translates known protocol behavior; it rejects unknown revisions as protocol changes. A general-purpose upstream bytecode interpreter is not required to preserve this implementation. Keep that fail-closed protocol-version behavior when porting. Current source/probe cache settings and separate playlist/segment budgets also differ from older architecture prose; source and tests are authoritative.

Keep media bytes, segment probing and relay delivery native. Components perform discovery, protocol parsing and provider-specific transforms. Validate component results against the request, provider identity, candidate cap, URL policy and credential scope before probing. Preserve required AES/RC4/ChaCha/GCM/custom VM behavior according to actual source and fixtures rather than a fixed algorithm list from the prompt.

Compile and warm components outside the lookup path. Cache compiled artifacts locally under trusted ownership; distribute portable Wasm and avoid accepting arbitrary precompiled native runtime blobs. Set memory/table/instance limits, execution interruption, request deadlines and cancellation on host calls. Benchmark epoch interruption versus fuel rather than assuming either is free.

Activation is per provider: verify and stage a new immutable version, perform compatibility/fixture checks, then atomically publish a registry generation for new lookups. Existing lookups retain their old version until completion or timeout. Pin the module version in diagnostics and cache namespaces; invalidate old discovery caches deliberately. Already registered relay sessions retain their fetch context and continue independently of provider activation. Keep the previous working version for recovery.

Release acceptance needs tests for incompatible modules, malformed outputs, infinite execution, blocked host calls, cancellation, memory exhaustion, interrupted install, concurrent update/lookup, and update during active playback. A fixture health check establishes contract validity, not that a changing upstream site works.

## Application updates

Use separate application and provider release streams. A provider update must not require reinstalling the daemon, migrating user data or restarting active relay sessions.

Use an established signed metadata system such as TUF, including trusted-root rotation, metadata expiry, version checks, artifact hashes/lengths and delegated provider targets. HTTPS and a checksum hosted alongside an artifact are insufficient trust anchors. Reject downgrade/replay attempts; an operator-authorized recovery to a retained local known-good package is a separate recorded action.

Headless UX can expose authenticated status/management endpoints plus CLI commands for checking, downloading and applying updates. The iOS client can later display these through the same management API. Default channels and automatic-install policy are product settings to decide before release, not assumptions to bake into the first models.

Stage platform/architecture-specific daemon releases in version directories. A small launcher or installer-controlled helper activates the next version after the server drains and exits. Windows executable replacement and OS service registration require platform-specific packaging adapters outside portable core logic. Do not promise uninterrupted playback when updating the single daemon; support deferring an application restart while sessions are active and a bounded explicit restart policy.

Health-check the new daemon and retain the previous package. Binary recovery is safe only if data/config schemas remain compatible. Prefer expand/contract migrations, consistent backups and explicit schema compatibility metadata; never automatically restore old databases over newly written user progress.

## Portability, exposure and safety

Relay portability blockers are concrete: relay/src/secret.rs rejects unsupported OSes and imports Unix permissions; session/store/mod.rs imports Unix filesystem APIs; main.rs uses Unix signals. network/guard.rs attests Mullvad and wg0, while network/transport.rs binds interfaces. Replace these boundaries and preserve SSRF checks, destination pinning, header scoping, budgets, cancellation and pooling.

Provide explicit direct, HTTP proxy and SOCKS5 proxy modes with documented DNS behavior. A configured proxy failure must return an error without silently switching to direct egress. Test redirect and destination policy under each mode, including remote DNS trust limitations. No tunnel discovery or attestation is required for the portable product.

The Go API currently exposes a household network trust model and disabled account/auth routes. Public distribution does not imply public unauthenticated Internet exposure. Start with loopback binding and an explicit authenticated pairing/access design before allowing remote management. Keep relay grant signing, audience/session ownership, revocation, expiration and auth_revision semantics.

Enforce unsafe prohibition and Clippy unwrap/expect/panic denial in owned production crates with workspace lint inheritance. This is a first-party code rule; SQLite/runtime/TLS dependencies may internally use native or unsafe implementations. Removing all unsafe/native code from the entire dependency graph is a different requirement. Avoid indexing and arithmetic paths that can panic despite passing these Clippy lints; use parser limits, checked operations and fuzz/property coverage where input is hostile.

## Performance acceptance

Rust removes neither network latency nor upstream challenge work. Establish a current baseline before claiming improvements. Preserve overlapping discovery/probing and progressive eligible startup; do not wait for all providers before beginning useful work. Keep request coalescing scoped to credentials and preferences, bounded work queues, reusable connections and cancellation that actually joins or aborts owned work.

Measure separately: API overhead; cache and coalescing waits; each provider's discovery/authorization; first eligible candidate and full selection; relay registration; player readiness; first frame. Compare cold, warm and prepared playback independently. Record p50/p95/p99, errors, CPU, peak RSS, bytes, task/connection counts and concurrency with identical fixtures and explicit live runs. Include one bad/slow provider and client cancellation in load scenarios.

For components, measure cold compilation, warm instantiation, host-call copies and crypto/parser time. Do not let download/compilation block playback resolution. Keep caches bounded by bytes and entries. Measure relay memory under slow readers and cancellation; retain streaming backpressure and range/If-Range semantics.

## Revised migration gates

1. Establish a usable workspace boundary for the existing relay, pin the reviewed toolchain/dependencies, add lint enforcement and native three-OS CI. Capture contract fixtures, state-schema inventory and reproducible performance baselines. No production cutover.
2. Complete core contracts with Go/iOS JSON parity and validation vectors: movie/TV identity, season zero, optional cross-IDs, candidate/fetch context, probes, track/device preferences, failure taxonomy and ranking results. Separate catalog DTOs from provider lookup coordinates.
3. Implement storage against real legacy schemas and lifecycle requirements; cover cache expiry/coalescing boundaries, encryption-key continuity, migrations, restart and concurrent progress writes.
4. Prove the provider interface/runtime/update lifecycle with one fixture-only component, including compatibility, resource limits and activation while requests run. This must precede porting all provider implementations.
5. Port media transport, parser/prober, eligibility/ranking and coordinator, then the five providers one by one with exact existing vectors and differential fixtures. Account for Go integer wrapping in custom cryptography explicitly.
6. Adapt the existing relay to portable transport/secret/lifecycle boundaries and integrate its library into the daemon; preserve its regression corpus.
7. Port catalog refresh/recommendation services and public API orchestration/authentication, preserving iOS contracts and durable state. Add configuration and management/update endpoints and graceful shutdown.
8. Package/install/update on macOS, Linux (including Raspberry Pi arm64), and Windows; exercise interrupted updates, signature failures, schema compatibility, recovery and active-session policies. Validate actual iOS playback, seeking, subtitles, quality controls, title switching and restart recovery before cutover.

At each implementation gate: format, warning-free workspace check, Clippy, substantive crate/vector tests and native OS CI. Report the evidence and unresolved platform acceptance before advancing. An empty test suite is not a completed gate, and a successful macOS build is not Windows/Linux proof.

## Current primary references

- Rust 1.99.1 release: https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/
- Rust ABI guarantees: https://doc.rust-lang.org/reference/items/external-blocks.html
- Wasmtime component plugin example: https://docs.wasmtime.dev/wasip2-plugins.html
- Wasmtime execution interruption: https://docs.wasmtime.dev/examples-interrupting-wasm.html
- Wasmtime security model: https://docs.wasmtime.dev/security.html
- TUF specification (1.0.37, dated 2026-10-08 when consulted): https://theupdateframework.github.io/specification/latest/

Library selection and exact version pins still require a dependency/advisory/platform review during implementation. This document recommends architecture; it does not claim a Wasmtime prototype, completed updater, fresh production benchmark or successful port.
