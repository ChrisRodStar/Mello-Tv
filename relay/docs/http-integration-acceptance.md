# Running provider HTTP service to production relay

Baseline: providers d44ae22, relay 3fadfa5; separate coordinated branches
`integration/providers-relay-acceptance`. No catalog/API/client/reference changes.
This effort adds the real private Go HTTP boundary and test-owned future-API
orchestration. It does not move resolution/signing into the relay or claim that
the public API has adopted either private contract.

## Runtime boundary and offline coverage

Go: authenticated `POST /internal/v1/resolve`; authenticated `/health/livez` and
`/health/readyz`. Request: `{version: providers.v1, context_key, lookup}`. Response:
unchanged providers.v1 Selection, including ordered backups and exact identities.
The schema/HTTP examples and operation limits belong in the providers repository.
An absent selected source is an explicit no-source 200 and cannot create a relay
session. `lookup.refresh: true` bypasses adapter-result cache; it does not change
ranking or introduce a provider preference.

`tools/service_acceptance.py --opt-in --config PRIVATE_CLIENT_JSON` owns future-API
work: call that endpoint, hand the real response to POST sessions, sign grants
using returned auth_revision, traverse the authorized stream, activate a stored
backup, resolve fresh sources and call source replacement. It can own a dedicated
provider child, stop/join it, check existing-session bytes while down, restart it
and check again. It optionally requests a single relay restart through the
acceptance owner's mailbox and checks the persisted session/entry afterward.
These checks prove bytes and control semantics, never player reload/resume.

Client JSON contains literal loopback host and provider/control/stream ports,
provider_bearer_file, control_bearer_file, lookup_file, owner_id, signing_key_file
(PKCS#8 Ed25519 DER), kid, openssl (trusted OpenSSL 3 executable), optional
provider_command (argv of dedicated Go daemon/config), provider_attest_command
(argv of providers-netcheck and existing Go ledger), failover/refresh booleans and
optional restart_file. provider_lifecycle false omits the intermediate provider
outage/restart while retaining owned startup/cleanup. sample_graph true selects
at most 24 HLS resources/four references per manifest and labels the result
graph_complete false; it makes no full graph-closure claim. No shell is used. All files are private and outside Git;
no credential or upstream URL is passed as an argument or printed. Signer inputs
are short-lived files in a mode-0700 directory, removed after signing. Test-owned
signing is a stand-in for the future API, not production authorization adoption.

The script has 120 s total wall time, 32 loopback control/provider requests,
192 aggregate stream requests and 32 MiB aggregate stream-body reads. Each graph
pass is further capped at 64 requests/8 MiB/30 s. No automatic provider retry or
format substitution occurs. A large or range-constrained graph can return
incomplete; the tool does not certify a graph after sampling a truncated prefix.
Failover/source replacement must return resume_required and a new generation.
It opens the new entry; only an actual player can prove timeline restoration.

Opt-in cross-language regression:

```sh
cargo test --locked --lib provider_http_uses_production_control_and_stream_listeners -- --ignored --nocapture
```

Requires the sibling providers checkout (including its fixture test), Go, Python
and OpenSSL 3. `FAMILY_STREAM_OPENSSL` can supply its absolute executable path.
Builds a disposable Go test binary with GOPROXY/GOSUMDB off. The Go fixture runs
the **production handler, HTTP listener and shared four-provider coordinator**;
only adapter/probe boundaries are synthetic. The script owns/restarts that child.
The Rust fixture runs production private control/stream listeners, verifier,
encrypted store and Delivery with the existing cfg(test) resolver/dial injection
pointing to synthetic bytes. Production contains no destination/guard exception.
HLS master retains separate audio/subtitle URIs; MP4 uses HEAD/three ranges.
Both exercise explicit backup, uncached refresh and actual Go-process outage/
restart without per-resource provider calls. Scoped Authorization reaches the
synthetic origin, while control bearer and playback grant never reach it.
Sessions are revoked, provider children joined, listeners/network/store joined.
This fixture is not native-adapter resolution or decodable movie evidence.

## Evidence matrix

H = historical evidence in stage-7.md, **not renewed in this effort**.
S = current synthetic protocol/process evidence. Open = required real evidence
not obtained. None of the historical evidence crossed the running Go HTTP service.

| Required evidence | Movie HLS | TV HLS | Anime HLS | Progressive MP4 |
| --- | --- | --- | --- | --- |
| Running all-provider HTTP resolution -> authorized relay bytes | Open; S boundary | Open; S exact coordinates | Open; S MAL/cross-IDs | Open; S boundary |
| Source byte access | H selected fMP4 graph prefix | H graph prefix | H graph prefix | H HEAD + 3 ranges |
| Decoded video and independent audio | H 30 s video + nonzero PCM | H same | H same | Open |
| Player startup | H Safari | Open | Open | Open |
| Temporal seek + resumed frames | H Safari 600 s; second late seek failed | Open | Open | Open; ranges are not seeks |
| Sustained playback | H ~60 s; representative longer run open | Open | Open | Open |
| Separate audio/subtitle associations | S graph; live availability open | Same | Same | Available embedded/external tracks open |
| Rendered subtitles / spoken language | Open / unknown | Open / unknown | Open / unknown | Open / unknown |
| Explicit backup activation / player reload-resume | S control+new entry bytes / Open player | Same | Same | Same |
| Uncached source refresh | S control+new entry bytes; live open | Same | Same | Same |
| Provider outage/restart during established session | S owned Go process and continued bytes; live open | Same | Same | Same |
| Relay drain/restart and client recovery from persisted state | Existing S drain/recovery tests; integrated live open | Same | Same | Same |
| Physical-device playback | Not authorized/unverified | Same | Same | Same |

Missing formats/tracks/protection cases stay unavailable, not passed. Non-silent
PCM never proves spoken language; track labels never prove rendered subtitles.
All final-head checks and review results are reported in the PRs/delivery report.
Overall playback acceptance remains open. After approval, the first live attempt
stopped during Go egress attestation, before provider resolution/media requests.
See the run outcome below and [fresh approval plan](http-integration-live-plan.md).

## Executed offline validation

Go formatting, go vet ./..., go test -race -count=1 ./... and daemon/netcheck
builds passed. An actual built Go daemon answered authenticated liveness and
joined SIGTERM shutdown using a synthetic private config without upstream work.
Rust fmt, locked all-target/all-feature clippy with warnings denied, and locked
all-target tests passed: 157 tests, with two opt-in cross-language tests excluded
by default. Both were explicitly invoked and passed. Python acceptance tooling:
28 tests passed. These results do not attest current Mullvad egress or playback.

Initial manual Greptile findings were reproduced/fixed: readiness retry after an
owner restart acknowledgment uses the original wall/request allowance; the
synthetic cross-language client now has an unreaped-leader/process-group owner
that kills/joins ordinary descendants at its deadline, with a timeout regression;
source refresh must increase generation, and an unchanged-generation regression
now fails. All required checks were rerun after these corrections. No live run.

For the four-title live proposal use a single manifest client with a top-level
`cases` array (1..4 per-case configs) and the shared `provider_budget_file`.
The batch runs cases sequentially with individual 120-second wall bounds, stops
on failure, and seals the original Go ledger once after cleanup. Never launch
four provider-owning cases concurrently or seal/recreate a ledger between them.
Two regressions verify ordering, no later case on failure and final sealing.

## First approved live attempt: blocked at prerequisite

Executed against providers `cdad617a861023f3f60ef0f252d01f51f4519363` and relay
`1e9d705659df72f8fabdca0a5bf89238f6e42e7e`. The owned operator stopped after Go's
guarded egress diagnostic returned failure. Runtime: 1.4 seconds. Go reserved
483,330 response-allowance bytes across five HTTP/DoH requests; Rust reserved
zero bytes and admitted zero HTTP exchanges. These are ledger reservations,
not exact wire/download counters. Both original ledgers were sealed; they will
not be reopened, reset or replaced to continue this run. No provider resolution,
media request, relay session, decoder, player or device check occurred. No
test client/daemon remained; ephemeral keys/configuration were removed.

The Go diagnostic used the generic attestation hostname for both forced families
and omitted family/step results. Its failure therefore does not establish the
precise blocker (including whether IPv6 DNS/routing or the exit assertion failed).
Go now matches Rust's explicit family-specific endpoints and emits only fixed
safe family/stage outcomes, with offline regressions. That correction remains
live-unverified. The whole live matrix stays open; stopped ledgers grant no
further traffic. A fresh run requires a separately approved allocation.

## Owned playback preparation (fresh live approval pending)

`tools/playback_acceptance.py` now composes the actual Go HTTP and relay control
paths with an owned native macOS AVPlayer client and independently counted FFmpeg
video/audio decoding. `tools/playback_operator.py prepare` is offline; `run` is a
separate explicit opt-in requiring the allocation in the live plan. Neither
command opens an original sealed ledger. No live matrix cell changes from this
preparation. Providers remain at dac13b74b16cb06c6934a639f8908e30d9d17609.

The explicitly invoked `playback_synthetic.py` generated tiny MP4 and HLS fixtures
locally, including opaque extensionless segments, independent HLS audio and WebVTT.
Both decoded 46 video frames/2 seconds and about 2 seconds nonzero PCM. The owned
native player displayed video-ready/frame progression, sought to 3 seconds and
resumed in both formats. HLS source cues produced white glyphs in the custom
subtitle overlay, verified by in-memory rasterization with no stored capture/text.
This is synthetic player/decoder tooling proof, **not production-listener decoding
or real-source rendered-subtitle evidence**. Native framework compatibility APIs
compile with SDK deprecation warnings; no SwiftUI/client project is changed.

The receive gate remains the production gate. The outer acceptance owner now
also watches the independent Go ledger to stop clients at its terminal state,
and seals Go after group cleanup, including when orchestration is killed.
Regression coverage includes that shutdown, failed-attestation sealing/no reuse,
preserving unrelated files and rejecting frame-free player success. Default Python
checks need neither FFmpeg/AVPlayer nor credentials/Mullvad; the native smoke check
is separately invoked. The original two synthetic production-listener tests remain
required. All current live layers remain open until fresh approved execution.

Manual Greptile review of the initial tooling head identified valid interrupted
attestation ownership, trailing video-freeze detection, accounting-error cleanup,
cleanup-report preservation and retained-hash issues. Corrections install a
non-throwing signal owner before allowance creation/diagnostic launch, retain
registered children through cancellation, measure the unfinished last-frame gap,
attempt each ledger/cleanup independently, preserve completed stage evidence and
retain executable/tool hashes. Default synthetic regressions exercise SIGTERM
while a diagnostic runs, damaged-ledger cleanup, failed session DELETE evidence,
and numbered restarts. The explicitly invoked native fixture additionally rejects
a real synthetic MP4 with one second video followed by an audio-only tail; time
advancement and earlier decoded frames no longer certify frozen final video.
Private cleanup failure or unconfirmed ledger sealing also forces a nonzero
operator exit, with CLI regression coverage. Final-head review completion is
recorded in the PR/delivery report.
