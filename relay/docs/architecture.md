# Architecture and ownership

## Responsibilities

The relay owns playback grants verification, source/session snapshots, encrypted
session/resource persistence, HLS parsing/filtering/rewriting, progressive/ranged
media delivery, upstream destination policy, bounded resource use, cancellation,
health and graceful process lifecycle. It does not resolve providers, rank them,
fetch catalog metadata, identify spoken language or transcode codecs.

The Go API owns user authorization and signs playback grants. It calls providers
with canonical identifiers, imports the selected source and ordered backups into
the relay, and returns an authorized relative `/stream/*` URL to clients. The
relay verifies every request independently without a per-segment API round trip.
Client profiles alone are not an authorization mechanism.

The providers own protocol-specific transformations, source lookup, probing,
selection and ordered alternatives. Their observed `providers.v1` JSON is the
input boundary. Library/recommendation/catalog state is unrelated to media
session lifetime. Existing sessions retain source snapshots during provider,
catalog or API outages, subject to source expiry and grant validity.

## Runtime design

One relay process, two listeners: private control/health and private data-plane
streaming. Only the latter is routed under `/stream/*` by Tailscale Serve. Local
listeners bind loopback; deployment binds use private container networking plus
host-loopback ingress. Never route `/internal/*`, metrics or state inspection via
Serve. Separate listener authentication from upstream source networking.

Use Tokio structured task ownership/cancellation, Axum HTTP routing, Hyper plus
hyper-util with a custom connector, rustls TLS, serde JSON, bounded tracing/metrics,
and relay-owned SQLite metadata persistence with application-level authenticated
encryption. These are implementation choices; crate versions/features and exact
APIs must be resolved when the manifest is created. The connector must own DNS
and socket creation rather than delegating to an unguarded default connector.

Persist source snapshots and resource registrations before returning client URLs.
Only a bounded hot index sits in memory; disk state contains encrypted sensitive
records, not media bytes. Do not share catalog or API database files. No global
mutex around network/store I/O. Batch store transactions per rewritten manifest;
resource reads do not produce a write/heartbeat per media segment.

## Planned Rust tree

The production modules below implement stages 1–6; live integration remains stage 7.

```
src/main.rs                  configuration, startup, signal handling, task ownership
src/lib.rs                   reusable production core for daemon and relayctl
src/config.rs                typed configuration and cross-limit validation
src/error.rs                 safe public errors and internal typed causes
src/auth/                    control bearer and offline Ed25519 playback JWT validation
src/control/                 relay.v1 session lifecycle endpoints and relayctl import
src/session/                 snapshots, source generations, resource map, encrypted store
src/network/                 destination policy, guarded resolver, bound connector, pool
src/hls/                     bounded parser, master filter, URI registry and serializer
src/media/                   streaming bodies, ranges, MIME and response header policy
src/health/                  liveness/readiness, drain state, bounded metrics
```

`network` is the sole upstream egress boundary for playlists, keys, subtitles,
init data, segments, MP4, redirects, reconnects and source validation. `hls` and
`media` cannot instantiate independent clients. Control listener traffic is a
separate private service connection and must never become an SSRF exception for
upstream source fetches.

## Design invariants

1. No upstream URL, context cookie, bearer or protocol token reaches a client URL,
   manifest, public error or log. Resource IDs are opaque, session/generation-bound.
2. No media egress exists before VPN attestation and interface binding succeed;
   policy transitions cancel old connections and prevent pooled-socket reuse.
3. Selected master and its associated rendition groups remain together. Backup
   changes create a new source generation, never remap an old resource ID.
4. No per-resource request requires a live provider, catalog or API process.
5. Data bodies are streamed; only bounded manifests, prefix inspection and control
   payloads may be aggregated. Slow clients impose backpressure.
6. Durable records and public grants have explicit expiry and revocation semantics.
   Unknown upstream expiry is not unlimited availability.
7. Component updates preserve process boundaries; whole-container replacement
   interrupts connections and needs explicit client recovery.

## Current integration boundary

The paired providers service defines authenticated POST /internal/v1/resolve around
its shared native engine. Test-owned orchestration passes the actual HTTP response
through production relay control/stream paths; [evidence and remaining gates](http-integration-acceptance.md)
distinguish synthetic service/process coverage from fresh live playback. Saved
selections remain regression evidence, not a replacement for that service boundary.
The public API must separately adopt both contracts. No resolver/signer is added
to relay production paths, and established sessions do not call providers/catalog.
