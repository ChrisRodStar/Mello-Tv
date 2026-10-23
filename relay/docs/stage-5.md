# Stage 5: progressive MP4 and bounded media streaming

Merged in [PR #5](https://github.com/ChrisRodStar/family-stream-relay/pull/5)
as `e96269b5666fb8724736cf932db2aa4411072b01` on 2026-10-15 with the user's approval.
The merged tree matches reviewed head `77462adcd7eb0f3df911685dadb77e497a287753`;
Greptile follow-up check 116586425252 completed with 21 files reviewed and zero
new comments. All three valid findings have fixes, regressions and resolved threads.

Implemented from main `b500904e14edf8514ba7830a8b65e80fa75718b5`, after stage-4
merge `c3faeca`. Implementation, offline gates, review and approved merge are
complete. Overall readiness remains **false**. Stage 6
and stage 7 have not been started. All work stayed local; no live source/media,
Server, deployment or system VPN/firewall operations were performed.

## Implemented delivery

- Authorized GET/HEAD entry and durable registered MP4 resources use the same
  guarded Hyper transport, prefix replay, exact byte-counting pull body and
  connection owner as HLS. No additional client, media cache/spool, whole-media
  collection, producer queue or post-commit retry was added.
- Closed, open and suffix ranges keep original offsets, totals and exact bytes.
  Malformed/duplicate/multipart Range and malformed/duplicate ranged If-Range
  fail before upstream work. Valid unsatisfiable ranges return 416 with the known
  representation total; ignored unconditional ranges fail explicitly.
- MP4 seek and small initial ranges use a bounded 16-byte first-box evidence GET.
  Sources need not support HEAD. Evidence and delivery compare total, available
  ETag/Last-Modified values and overlapping prefix bytes. Constrained HLS discovery
  can fall back from unavailable HEAD to a one-byte GET inside its capability.
  No requested extent is clamped to an internal evidence window or rebased.
- If-Range is forwarded through the existing redirect/credential policy. Strong
  ETags compare exactly; weak tags never match. Dates accept IMF-fixdate and both
  obsolete formats, require exact modification-time equality and intermediary
  date-strength evidence. Fresh full 200 fallback is allowed only for an
  unconstrained mismatching conditional; HLS capabilities cannot widen that way.
- Media MIME follows validated container/context evidence. Syntactically valid,
  safe unchanged-resource validators are preserved; source-identifying/private
  ETags, hop fields, cookies, redirects and diagnostics are stripped. Full MP4
  and HLS prefix bytes are replayed exactly. Header/range/length mismatches fail
  before headers; observable short/overlong bodies and changing trailer metadata
  abort streaming without appending an error document.
- MP4 uses `mp4_max_transfer_s` rather than the HLS segment total. Transfer policy
  is clipped by grant, session, generation/resource and known source expiry.
  The real downstream connection now watches the VPN epoch and transfer policy
  independently of body polling, in addition to authorization cancellation.
  Existing finite listener/transport/store admission and owned task shutdown stay
  in place. Expired admission waits report request_limit rather than upstream idle.

## Offline verification

Executed locally on 2026-10-15 with the committed lockfile:

```
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
```

All passed: **122 offline tests** (75 library, 3 authorization, 15 foundation,
4 loopback, 25 session). All 106 baseline tests remain. Sixteen added cases cover
production MP4 entry/resource GET/HEAD and full/ranged bytes; If-Range ETags/dates,
fallback/capability isolation; unavailable HEAD; malformed/unsatisfiable/ignored
ranges; inconsistent metadata and changed evidence; MIME/prefix replay;
truncated/overlong bodies and trailers; global/session/authority admission;
read idle; slow consumption; and real-listener cleanup while clients stop reading
on guard loss, revoke, transfer deadline, shutdown, disconnect and write idle.
Existing HLS graph, rendition, encrypted key/media, subtitles, absolute capability,
durable identity/generation and all stage-4 review regressions remain included.

## Bounded local measurement

Mac arm64, rustc 1.99.0, debug test binary; synthetic loopback sockets through the
existing `cfg(test)` resolver/dial injection. No production security bypass exists.
The fixture advertises **1 TiB per stream** and generates reusable **16 KiB** chunks
only as socket writes complete. It never allocates a 1 TiB representation.

Isolated invocation after building the locked test binary:

```
/usr/bin/time -l target/debug/deps/family_stream_relay-2dc9ad2a61f5001f \
  network::transport::tests::stage4::stage5::mp4_large_logical_slow_reader_is_bounded_and_admission_recovers_on_drop \
  --exact --nocapture
```

Observed sample: three simultaneous slowly consumed streams, **4,300,101 bytes**
consumed; **9,568,256 bytes** accepted by synthetic upstream socket writes. After a
1.1-second downstream pause, an additional 100-ms observation showed **0 bytes**
of write growth. The test permits OS scheduling/socket variance and asserts less
than 32 MiB total fixture writes at this observation, rather than treating this
sample as a portable kernel-buffer bound. It then holds eight streams, verifies
ninth-request session admission fails without a new dial, drops bodies, observes
all eight source sockets close, and checks all 32 global permits and per-key
budgets recover. Runtime: **1.37 seconds**; process maximum resident set size:
**19,628,032 bytes**; macOS reported peak memory footprint: **4,473,360 bytes**.
These include the test harness, SQLite and runtime; kernel socket memory is not
included. This is a bounded synthetic experiment, not a deployment memory budget.

Applied defaults: global/session/authority streams **32/8/8**, admission wait
**2 s**, application frame cap **64 KiB**, sniff policy **4 KiB** (MP4 uses 16 bytes),
Hyper read buffer cap **32 KiB** and 64 headers; driver queue and active drivers
each at most twice the global stream limit. HTTP listeners cap owned connections.
Each pull body retains only its prefix, one pending upstream frame/future and a
possible final frame awaiting EOF, plus bounded Hyper buffers. There is no eager
media channel. Byte retention does not grow with the advertised object length.
OS socket buffers, TLS/runtime/allocator/store overhead are additional to these
application caps; RSS above is measured rather than derived by multiplying caps.

Read/write idle defaults are **20/30 s**; playlist/segment totals **20/120 s**;
MP4 absolute maximum **21,600 s**. Tests use shortened idle/transfer settings for
cancellation checks and prove an active MP4 outlives a one-second HLS segment total.
No viewer-capacity extrapolation or sustained real media throughput claim is made.

## Limits and later acceptance

Container evidence does not decode video/audio or prove language/content identity.
A server that changes same-length, nonoverlapping bytes while omitting validators
(or lying consistently about them) cannot be detected by bounded prefix inspection;
no whole-object hash is computed. Observable totals, validators, overlaps, framing
and EOF are checked, and no independently fetched media windows are assembled.
Conditional range evidence can temporarily need two transport slots and fails
admission if policy cannot admit the second; it never bypasses a limit.

Actual upstream compatibility, decoder/player startup/seek/audio/subtitles,
sustained playback, live Mullvad loss/no-bypass, deployment/Serve, devices and
component replacement remain unverified. Stage-6 operational/failover work and
stage-7 acceptance remain open. Exact reviewed commit/outcomes are recorded above and in the PR
review records and delivery report.

Primary semantics: [RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html), especially
sections 8.8, 13.1.5 and 14. Existing HLS semantics remain
[RFC 8216](https://www.rfc-editor.org/rfc/rfc8216.html).

## Review corrections

Initial Greptile review of `371c723deffa650e76a2a684101fb541bab157ae`
(check 116581282208) completed with three valid findings. All three have fixes
and regressions: 64-bit extended-size BMFF box headers are accepted with checked
header length/size; conditional EOF 416 responses can omit validators and use
bounded independent metadata (including HEAD-unavailable sources) to confirm
condition/total; cancellation tests give unrelated write-idle timers a duration
longer than their assertion window. The same RFC validator-omission rule is
covered for conditional 206 responses without publishing invented headers.
The PR records the follow-up review of the final exact head and resolved threads.
