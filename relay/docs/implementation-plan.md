# Production relay implementation plan

See the [stage TODO](../TODO.md) for the seven-stage overview and current progress.

Checkmarks record implemented stage scope; runtime limits are documented in
[stage-1 evidence](stage-1.md). This is the full relay delivery
scope, not a throwaway preview or a replacement provider resolver. Keep scope
bounded by the documented HLS/MP4 contract rather than adding unrelated services.

## 1. Runnable Rust foundation and exact wire boundary

- [x] Create Cargo.toml/Cargo.lock, main/lib, typed config/errors, Tokio task owner,
      Axum private control and streaming listeners, safe logging and signals.
- [x] Implement schema-compatible providers.v1/relay.v1 Rust models and import
      validation from current Go-produced golden fixtures, preserving unknowns.
- [x] Implement finite admission, body/header/state limits and startup invariants.
- [x] Add relayctl selection importer that calls the real private session endpoint
      with secret-file credentials; no second preview-only stream path.
- [x] Resolve compatible maintained crates from primary docs, pin features/lockfile,
      and document MSRV/platform support when the manifest is committed.

Stage 1 gate: actual Go-produced envelopes parse; real loopback control/importer
regressions pass. Session creation deliberately returns 503 until stage 3 and
readiness remains false. Transport/store startup invariants arrive in stages 2/3.
No upstream/media request has been made.

Gate: parsable actual Go envelopes and private control foundation; no fake provider
HTTP endpoint, source URL exposed to clients, or unsupported-format relabeling.

## 2. Controlled transport and authorization boundaries

- [x] Implement public-destination parsing and post-DNS/socket address policy.
- [x] Implement tunnel-bound local Darwin DNS/sockets and VPN epoch ownership.
- [x] Implement Hyper/rustls connector, bounded pools/timeouts, manual redirects,
      credential rebuilding, no decompression/proxy bypass or default-client escape.
- [x] Implement continuous guard loss cancellation and stale connection invalidation.
- [x] Implement private bearer control auth and pinned EdDSA playback grant validation.
- [x] Cover rebinding/redirect/header and token/resource ownership attacks offline.

Gate: no upstream path can bypass the guard/connector; authorization is checked
for GET/HEAD and every nested resource without a per-segment API dependency.

Implementation and offline/live egress evidence: [stage 2](stage-2.md).
Durable authorization is implemented in [stage 3](stage-3.md); media delivery
remains unavailable.

## 3. Durable sessions, resource identity and lifecycle

- [x] Create relay-owned SQLite store, migrations, encrypted snapshots/resources,
      key IDs/nonces/AAD, revocations and bounded idempotency entries.
- [x] Implement session admission/expiry/renew/revoke and transactional source revisions.
- [x] Implement opaque stable generation-bound resource IDs, bounded hot cache,
      durable registration batches and cleanup without per-segment writes.
- [x] Implement redacted control reads, state-store failure behavior and crash restore.
- [x] Cover ID stability, corruption/wrong keys, CAS/retries and bounded task shutdown.

Implementation and offline evidence: [stage 3](stage-3.md).

Gate: issued URLs have durable recoverable mappings; signed grants are not mistaken
for persistence or survival of an interrupted TCP connection.

## 4. Complete HLS delivery

- [x] Parse bounded masters/media playlists and quoted attributes with a declared
      supported-tag/version matrix and explicit unsupported extension errors.
- [x] Filter selected video child while retaining complete associated rendition groups.
- [x] Rewrite every supported fetch reference to authenticated same-origin resources.
- [x] Implement init/key/subtitle/segment kinds, sequence/discontinuity preservation,
      AES-128 pass-through, range state, nested graph/cycle limits and prefix sniffing.
- [x] Commit registrations before manifest output and preserve consistent GET/HEAD.
- [x] Reuse behavioral cases from Go handoff/probe fixtures without copying the
      preview's full-body media buffering or regex-only parser as production design.

Gate: master/audio/subtitle/key/init/resource closure with no upstream URL leakage
or loss of rendition attributes; malformed/ambiguous linkage fails explicitly.

Implementation, review and merge evidence: [stage 4](stage-4.md). Stage 4 is
merged as `c3faeca`; readiness remains false and stages 5–7 remain open.

## 5. Progressive MP4 and bounded media streaming

- [x] Implement pull-based body streaming, finite slots/chunks, backpressure,
      client cancellation, read/write idle timeouts and grant/session bounds.
- [x] Implement accurate single-range GET/HEAD and If-Range behavior, checked
      arithmetic, valid 206/416 headers and representation-consistency checks.
- [x] Preserve resource-specific MIME and validated unchanged response headers;
      strip unsafe framing/cookies/redirects and do not collect full media bodies.
- [x] Verify low-memory behavior using bounded accounting and authorized measurement,
      not an assumed concurrency/viewer capacity claim.

Gate: seek-capable MP4/HLS delivery with exact advertised extents; no relabeling of
errors, unbounded buffering, eager queued downloads or media spooling.

Implementation, offline proof and bounded measurement: [stage 5](stage-5.md).
Merged in [PR #5](https://github.com/ChrisRodStar/family-stream-relay/pull/5) as
`e96269b` after user approval. Final review of `77462ad` added zero new comments;
overall readiness remains false.

## 6. Failure recovery and operational integration

- [x] Implement ordered backup preparation and explicit generation activation/CAS.
- [x] Require caller reload/resume for source switches; never splice timelines.
- [x] Implement source-refresh contract, safe failure taxonomy and diagnostics that
      distinguish upstream errors, timeouts and exhausted local resource budgets.
- [x] Implement private liveness/readiness/metrics, relay/provider health separation,
      graceful bounded drain, store flush and restored sessions after restart.
- [x] Document component artifact activation/rollback and encryption/store migration.
- [ ] **Deferred, separate request required:** prepare streaming-container
      supervision/firewall packaging only when requested;
      preserve the established exactly-three-container layout.

Gate: existing source sessions do not depend on live providers/catalog/API per
segment; failure/restart effects and exit-IP source invalidation are explicit.

Implementation and offline/lifecycle evidence: [stage 6](stage-6.md).
Stage 6 merged as `68ce3dcb8792fbb275201982458c850fc015b089`.
Packaging is explicitly deferred; stage-7 production acceptance remains open.

## 7. Cross-component and playback acceptance

- [x] With authorization, pass full offline fmt/clippy/tests and meaningful protocol
      fixtures before live testing. Keep default tests offline.
- [ ] Exercise the actual Rust relay for movie/TV/anime HLS and progressive MP4,
      decoded video/audio, actual player startup/seek/subtitles and sustained playback.
- [ ] Exercise provider-only replacement, relay drain/recovery, explicit backup
      reload/resume and API/catalog outage with existing grants.
- [ ] When separately authorized, verify actual local tunnel loss/no bypass and
      eventual streaming-container/Serve/device behavior in its real network context.
- [ ] Update evidence with exact commits, settings and verified/unverified layers.

Completion requires the production service and its documented failure properties,
not a scaffold, successful compile or short preview. Stage 1 foundation completion does not establish any playback or production
runtime acceptance item.


Stage-7 tooling, executed offline evidence and the acceptance/blocker matrix are
recorded in [stage 7](stage-7.md). Its live/player/device gates remain unchecked;
the approved [local live plan](stage-7-live-plan.md) has ended after a harness
byte-budget overrun. Remaining live checks need fresh approval and a corrected
receive-boundary aggregate byte gate.
