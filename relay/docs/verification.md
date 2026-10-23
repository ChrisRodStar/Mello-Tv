# Verification and acceptance requirements

Stage-specific executed evidence is recorded in [stage 1](stage-1.md),
[stage 2](stage-2.md), [stage 3](stage-3.md), [stage 4](stage-4.md),
[stage 5](stage-5.md), [stage 6](stage-6.md) and [stage 7](stage-7.md).
The stage-7 matrix distinguishes existing synthetic coverage from open playback
requirements. Use its opt-in tooling and approved plan for live checks.

## Offline matrix (default suite never contacts real providers)

| Area | Required evidence |
| --- | --- |
| Go handoff | Go-emitted providers.v1 golden JSON parses in Rust; movie/TV specials, optional fields, nanosecond durations, ordered backups, unknown metadata |
| Control API | schema/body limits, bearer auth, idempotent create, conflicting keys, CAS mutations, store failure before publication, redacted errors |
| Grants | EdDSA issuer/audience/alg/kid, required/duplicate claims, expiry/nbf/skew, wrong owner/SID/revision, duplicate query grants, every resource kind + HEAD |
| SSRF | literal/private/mapped IPs, mixed DNS answers, rebinding, local suffixes, credentials, ports, redirects, second-resolution and happy-eyeballs paths |
| VPN | guard initially down, epoch/interface changes, active body cancellation, stale pooled connection rejection, DNS never falling back |
| Credentials | exact origin/port/path boundary and cookie path; public CDN identity separate; CSRF/Authorization/Cookie never cross origins |
| HLS | selected master retains audio/subtitles/captions and all attributes; missing/ambiguous groups, quoted commas, relative/final URLs, nesting/cycles, registry reuse |
| HLS resources | MAP, AES-128 key changes/IV/METHOD=NONE, subtitle/iframe policy, explicit/implicit byte ranges, unsupported URI extensions rejected |
| Containers | genuine TS/fMP4 under wrong MIME; reject HTML/JSON errors; prefix replay has no loss; WebVTT checked; unknown DRM rejected |
| HTTP ranges | GET/HEAD, start/end/open/suffix, overflow, 416 vs 400, no multipart, If-Range 200, ignored Range, mismatched 206 length/total, changing ETag |
| Streaming | slow reader backpressure, bounded chunks/queues, aborted client, idle timeout, revoked/expired grant, body-count mismatch, no media disk writes |
| Sessions | encrypted durable records, unique nonces/AAD, wrong key/corruption, stable IDs after restart, expiry/tombstones/cleanup, idempotency recovery |
| Failover | provider order, explicit generation change/reload, CAS conflict, old resources never serve another provider's bytes |
| Lifecycle | drain admission, bounded shutdown, joined tasks, store failure, full state limits, process restart isolation |

Use injected fake resolver/socket/clock/store/HTTP transport boundaries, not an
unsafe production switch. Local synthetic servers may be used only through test
transport injection; production SSRF rules remain enforced. Concurrency tests
must prove work exits, caps are respected and completed work is not overwritten
by a timed-out alternative. Do not merely mirror private function implementation.

When Rust code exists and verification is authorized, run:

```
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
```

Add fuzz/property cases for HLS attributes/URI closure, ranges/overflow and context
scope canonicalization when justified. Lock dependency versions; offline CI should
separate compilation/dependency acquisition from tests that exercise protocols.
No checked-in fixtures contain real tokens, signed URLs, keys or media.

## Authorized live local acceptance

Use the actual Rust production relay/control path and the current Go engine.
Query all four providers without fixed priority; save private selections outside
Git, import them via relayctl/private control, and attest Mullvad from relay context.
Do not count Go preview playback as Rust relay proof.

Exercise movie, exact TV episode and anime selection; clear/fMP4/AES-128 HLS,
separate audio/subtitle renditions, and progressive MP4 seeking where available.
Include a long media playlist (prior observed source exceeded 512 KiB), byte ranges,
keys/init resources and public Referer/scoped-header-dependent CDN requests.

Record separately:

1. Source resolution and selection (provider/coordinates, unknown metadata).
2. Upstream manifest/media byte accessibility and exact range behavior.
3. Automated decoded video and independently decoded, non-silent audio.
4. Actual Safari and authorized iOS/AVPlayer playback, startup and seeking.
5. Language/subtitle evidence; labels and nonzero PCM do not prove spoken language.
6. Sustained playback/representative late seek positions, not just first frames.
7. Provider process restart while existing relay sessions keep requesting resources.
8. Relay drain/restart with durable stable URLs and documented client retry/resume.
9. Explicit backup activation with new source generation and player reload.
10. API/catalog unavailability during existing authorized session delivery.

A separate explicitly authorized tunnel-loss experiment must confirm no source
packet bypasses Mullvad on DNS, IPv4, IPv6, pooled or in-flight connections. Do not
disconnect the user's tunnel as an incidental setup/check step. Production
container/Serve/physical-device verification is separate from local process proof.

Do not claim guaranteed viewers, bandwidth capacity, invisible cross-provider
switching or uninterrupted container replacement from these functional checks.

Stage-5 implementation/offline tests and the local bounded synthetic measurement
are recorded in [stage 5](stage-5.md). They do not establish any stage-7 live,
player, decoder, tunnel-loss, Serve or device acceptance item.
