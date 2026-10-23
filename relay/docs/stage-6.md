# Stage 6: failure recovery and operational integration

Implemented from synchronized main `255ae31`, after stage-5 merge
`e96269b5666fb8724736cf932db2aa4411072b01`. Stage 6 merged in [PR #6](https://github.com/ChrisRodStar/family-stream-relay/pull/6)
as `68ce3dcb8792fbb275201982458c850fc015b089` with user approval. Runtime component readiness is now meaningful;
overall live/player/device/deployment acceptance remains **false**. Stage 7 evidence is tracked [separately](stage-7.md). Container/supervisor/firewall packaging is explicitly deferred until
a separate request. All work and fixtures stayed local on the Mac; no live
source/media requests, Server, deployment or system VPN/firewall changes occurred.

## Implemented production paths

- Authenticated failover reads the stored owner, expected revision and original
  ordered backup snapshot. It prepares candidates with the sole guarded transport
  and shared HLS parser/MP4 delivery validator. It never ranks providers or asks
  providers/API/catalog for a segment. Expiry, canonical title/cross-ID/episode,
  protection/format, FetchContext and selected master/rendition linkage checks
  remain enforced. Source refresh prepares the API-supplied selection through the
  same path; the API/providers remain responsible for obtaining it.
- Preparation runs outside the store owner. The existing encrypted source/CAS
  transaction atomically publishes the new root, prepared HLS graph registrations,
  session revision, source generation and old-generation grace. Store failure,
  lost CAS and cancellation/epoch/deadline checks before commit leave the active
  generation intact. No temporary generation/resource rows, media collection,
  spool, second upstream client, recovery queue or retry table were added.
- Failed source observations do not mutate the active backup order. A retry
  repeats bounded preparation; a successful failover persists the remaining
  suffix in the new encrypted snapshot. This makes restart behavior bounded and
  avoids a permanent blacklist from transient observations. CAS prevents a retry
  of a lost successful reply from causing another switch; GET resolves its outcome.
- Every switch returns revision, generation, format, relative entry path and
  resume_required true, preserving auth_revision. Entry-time fallback is not
  implemented for GET or HEAD. Old resource IDs retain their original generation;
  old manifests may reuse existing registrations during grace, but cannot add new
  old-generation mappings. Failed media never triggers another source at its URL;
  post-commit errors terminate the response without retries or appended JSON.
- Source-specific rejection may advance through backups. VPN failure, cancellation,
  timeouts and local admission/byte/state exhaustion remain distinct and stop the
  operation. Exhausted alternatives return source_refresh_required. A connector
  that runs out of addresses after a per-address timeout now retains timeout
  classification instead of becoming an apparent source rejection.
- Private bearer-authenticated liveness, runtime readiness and metrics inspect
  actual store/key owner, guard epoch, verifier, task owner, admission and drain
  state without fetching media. Provider new-playback health remains external;
  provider loss cannot disable otherwise usable existing sessions. Metrics use a
  fixed vocabulary, saturating process-local counters and stage timing sums,
  fewer than 64 series/8192 bytes, with no URLs, secrets or identifiers. Startup
  correctly reports authenticated HLS and MP4 support and pending overall acceptance.
- The daemon owner stops listener admission first, permits finite active drain,
  aborts/joins remaining connections, then cancels/joins transport drivers and
  guard monitor, and finally shuts down the durable store worker. Revocation and
  guard enforcement continue during drain, even for unpolled downstream clients.
  Infrastructure task failure marks health failed and starts bounded shutdown.

Applied policy: eight-second recovery deadline inside ten-second control boundary;
five-second expiry margin; at most stored backup count (127 default); aggregate
1 MiB accepted manifest input; 4096 manifest registrations and 2 MiB metadata batch
(defaults), existing depth/state/disk admission. MP4 uses 16-byte prefix evidence
plus bounded transport-frame read-ahead per attempt. All preparation shares the
existing control/transport admission budgets. See failover.md for exact semantics.

## Offline verification

Required commands, using unchanged dependencies and retained Cargo.lock:

```
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
```

**143 offline tests pass**: 96 library, 3 authorization, 15 foundation, 4 loopback,
25 session. All **122 baseline tests remain**. Twenty-one added tests exercise:
ordered failed/successful candidates and exhaustion/retries; interrupted preparation,
VPN loss, timeout and local admission/byte/state budgets; concurrent failover and
source refresh; owner/revision/identity conflicts, source expiry and revocation;
transaction rollback after root/session changes but before all resource inserts;
pre-commit cancellation/epoch/deadline rollback; restored HLS graph mappings and
MP4 generation IDs/revocations; no automatic GET/HEAD or post-commit substitution;
HLS-to-MP4 refresh/resume; component health/provider separation and redacted bounded
metrics; graceful versus forced real-socket drain, permit/socket/budget cleanup;
ordered infrastructure shutdown and failure detection during drain/simultaneous owner exits; and per-address
connect timeout classification. Existing abrupt-process-exit, store corruption/key,
range/If-Range, HLS rendition, backpressure and cancellation regressions remain.

## Bounded synthetic lifecycle measurement

The real HTTP listener/production delivery path uses the existing cfg(test)
resolver/dial injection. Its upstream is synthetic loopback. One finite MP4 is
paused after its validated prefix; shutdown begins, infrastructure remains healthy,
then the rest is released and the exact response completes. Another advertises
1 TiB and generates reusable 16 KiB writes; the downstream stops reading. With a
one-second drain deadline and unrelated read/write idle timers at ten seconds,
forced shutdown must finish within two seconds and close the upstream socket.
The assertion observes all 32 global permits returned and empty per-key budgets,
and bounds fixture writes below 32 MiB without allocating the advertised object.
No shortened unrelated timer can cause the expected one-second drain cancellation.

Isolated measurement after building the locked debug test binary:

```
/usr/bin/time -l target/debug/deps/family_stream_relay-2dc9ad2a61f5001f \
  network::transport::tests::stage4::stage6::active_drain_completes_accepted_transfer_and_forced_deadline_cleans_unpolled_sockets \
  --exact --nocapture
```

Observed sample: graceful completion **103 ms**, forced deadline **1003 ms**,
**2,375,680 generated upstream bytes** in the forced case; both observed one source
socket close, 32 available permits and empty per-key budgets. Total test runtime
**1.22 s**; maximum resident set **20,299,776 bytes**, macOS peak memory footprint
**4,309,544 bytes**. Measurements include the harness/runtime/store and exclude
kernel socket memory; samples vary with scheduling/socket buffers. They are not
viewer capacity, production memory budgets or sustained-throughput claims.

## Operations and limits

Operations documents independent artifact activation/rollback, three eventual
application-stack containers, Tailscale Serve as the single private entry point,
component supervision/restart responsibilities, bounded drain and persistence.
Schema version 1 and encrypted blob/AAD layouts remain unchanged. Startup still
validates all records/keys/bounds; a restarted process starts with no attested
transport epoch. Historical encryption keys and consistent state backups are needed
for recovery. Future schema/encryption migrations require explicit versioned,
transactional, bounded upgrades and a proven rollback plan. Restoring an older
backup must not silently restore revoked authorization.

Preparation proves bounded framing/manifest compatibility at that time; it does
not prove media identity, codec decoding, language, future source validity or player
resume. Source URLs can expire or become invalid after an exit-IP change. TCP
connections do not survive process replacement; whole-container replacement is
disruptive. Actual live Mullvad loss/no-bypass, source compatibility, player reload/
seek/audio/subtitles, sustained playback, Serve, devices and deployment are unverified
stage-7 acceptance layers. Packaging is deferred, not marked complete.

Review evidence is recorded in the PR and final delivery report with its exact
reviewed head. Silence or an unfinished check is not review completion.

## Review corrections

Greptile review of `5a7b9a4803c3e8e9ab8b6cd2b5e3e2f4686afce8`, check
116715772441, completed with 31 files reviewed and two valid findings. Both have
production-path fixes and regressions: diagnostics now have one independent bounded
permit, so a healthy relay with only one control permit remains ready and diagnostic
access survives control saturation; HLS preparation reserves auth.max_grant_bytes
for every rewritten reference, rejecting output-budget exhaustion before activation.
The regressions cover idle/saturated single-permit readiness, diagnostic admission
limits, and a manifest that fits without grants but must fail with the maximum
supported grant. Final-head follow-up review is recorded in the PR/delivery report.

The existing incomplete-control-body drain regression now probes another control
route for saturation instead of liveness, since diagnostics intentionally have
independent admission. Its drain, cancellation and join assertions remain intact.


## Completed final review and merge

Final-head Greptile check **116719368814** completed successfully on
`647512fb3e6e3a015ca15cbca7a475f350580951`: **32 files reviewed, zero comments
added**. The two earlier valid findings were fixed with regressions before that
review. Merge commit is `68ce3dcb8792fbb275201982458c850fc015b089`.
This updates review/merge status only; the 143-test and synthetic measurements
above do not become production playback acceptance.
