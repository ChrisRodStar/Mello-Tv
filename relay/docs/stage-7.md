# Stage 7: cross-component and playback acceptance

Started from clean synchronized main at stage-6 merge
`68ce3dcb8792fbb275201982458c850fc015b089`, on branch `stage-7-acceptance`.
Tracked changes stay in this repository. Stage 7 production acceptance remains
**open**. After the user approved the saved local plan, production egress,
provider imports, decoded HLS and one Safari movie playback/seek check ran.
The run stopped after the experimental late-seek decoder exceeded its byte
budget; see the incident/limits below. No tunnel disruption, physical device,
packaging, Server, Serve or deployment checks ran. Runtime readiness is separate
from the intentionally false overall acceptance flag.

## Existing evidence and missing checks

| Acceptance layer | Evidence available | Remaining acceptance / blocker |
| --- | --- | --- |
| Provider handoff | Actual Go-emitted synthetic movie/TV-special/rendition/MP4/ordered-backup fixtures parse; 143 stage-6 baseline tests retained | Fresh movie/TV/anime selections from all four adapters imported through production control; API integration remains separate |
| Guarded source access | Stage-2 historical IPv4/IPv6 production egress attestation; offline rebinding/redirect/guard-epoch regressions | Fresh production IPv4/IPv6 Mullvad attestation passed; real tunnel disruption remains unverified |
| HLS closure | Baseline master/audio/subtitle/key/init/range/TS/fMP4/AES-128 protocol regressions; new tool traverses synthetic HLS through real Rust sockets | Real selected fMP4 HLS masters/init/video delivered; rewritten movie child 802,506 bytes; separate audio/subtitle/AES-128 cases remain open |
| MP4 byte ranges | Baseline full/HEAD/range/If-Range/416 consistency tests; new tool samples start/middle/end synthetic bytes through production listener | Real isolated VidLink MP4 HEAD plus first/middle/end 64 KiB ranges passed; MP4 decoder/player/temporal seeks remain open |
| Decoded video/audio | All three selected HLS cases decoded 30 s video and independent nonzero PCM | FFmpeg 7.1 installed in ignored local tooling; late decoder seek timed out, MP4 decoding and spoken language unverified |
| Player startup/seek/audio/subtitles | Safari movie startup, 600 s seek and resumed frames observed | TV/anime/MP4 player and rendered subtitle/audible-language checks remain open; physical devices separately authorized |
| Sustained delivery | Baseline bounded slow-reader/cancellation and finite lifecycle measurements | Safari movie ran about 60 s including seek; representative longer sustained playback remains open |
| Explicit failover/resume | Baseline ordered guarded preparation, transactional generation/CAS, old-resource isolation, resume_required | Live backup activation/player reload not reached before the byte-budget stop; offline evidence is not player resume |
| Drain/restart | Baseline real-socket graceful/forced drain, cleanup, encrypted-store recovery, stable mappings and abrupt subprocess exit | Development daemon restarts retained session URLs, then decoding succeeded; active-stream drain/restart/client retry was not performed |
| Component independence | Existing sessions are served with no API/catalog/provider services in synthetic tests; persisted selections and grants suffice | Actual API adoption and provider HTTP service are not implemented boundaries here; no real replacement/outage integration proof |
| Tunnel loss/no bypass | Offline guard cancellation and stale-pool/DNS tests | Actual disruption/packet observations require separate authorization |
| Packaging/Serve/deployment | Operations contract only | Explicitly deferred; separate authorization and real network/device context required |

Read-only inspection confirmed the Go all-provider diagnostic is
`../family-stream-providers/cmd/vidfast-check` without `-vidfast-only`. It uses
all four native adapters and the shared engine with a 45-second bound, attests
provider Mullvad egress and saves a private selection. No provider repository
files were changed or invented REST endpoint added. The API must still adopt
relay.v1; absent services in synthetic tests do not demonstrate real outages.

## Prepared opt-in tooling

`relayctl import-selection` still calls the actual private session endpoint. An
optional final argument saves its typed SessionReady response to a new mode-0600
file (existing files/symlinks rejected). The receipt provides the SID, entry path
and auth_revision needed by a trusted signer; no upstream URL/context or token
is printed. The original invocation and status-only stdout remain compatible.
The operator must use a private directory outside Git. A failed receipt write
can follow a successful import: retry the identical payload, expiry and
idempotency key with a new receipt pathname to recover the same session. Do not
retry with a changed expiry under that key.

```
relayctl import-selection 127.0.0.1:8091 BEARER_FILE SELECTION_FILE OWNER_ID EXPIRES_AT IDEMPOTENCY_KEY NEW_PRIVATE_RECEIPT_FILE
python3 tools/acceptance_probe.py --opt-in --receipt PRIVATE_RECEIPT_FILE --grant-file PRIVATE_GRANT_FILE
```

The signer uses the actual returned session ID, owner and **auth_revision**, not
control revision; see sessions.md. This tooling does not impersonate the public
API or claim that the API contract has been adopted.

The Python standard-library probe connects only to a literal loopback stream
listener; it has no upstream HTTP client, DNS, proxy inheritance or redirect
following. Every discovered URI must be an opaque same-session `/stream/*` path
with either no query or exactly the supplied grant. No external/local alternate
host, other session, traversal, fragment, unrelated query or duplicate grant is
accepted. Inputs are bounded regular mode-0600 files opened without symlinks.
No media is written to disk. Output is aggregate counts or a fixed safe failure;
raw exceptions/URLs/bodies/identifiers/grants are suppressed.

Each CLI run has a 120-second Unix signal-enforced wall deadline (including
blocking header/chunk parsing), ten-second-or-less socket waits,
64 requests and 8 MiB total received body bytes. Each manifest is bounded at
1 MiB (one extra byte may be read to detect excess); non-manifest responses are
sampled at 64 KiB then closed. HLS breadth traversal validates discovered graph
closure within those bounds and samples leaves; large graphs fail incomplete
rather than claim partial success. MP4 HEAD plus three absolute range samples
check status/extent/body length. Neither prefix cancellation nor byte offsets
establishes sustained delivery or temporal seeks. The tool deliberately rejects
HLS range-constrained leaf requests that require Range; use a separately reviewed
range-aware player/check for those cases. It does not implement a second relay,
media cache, decoder, production bypass or automatic failover.

## Executed offline validation

Required Cargo fmt/clippy/locked all-target checks passed: **148 offline tests**
(99 library, 2 relayctl, 3 authorization, 15 foundation, 4 loopback, 25 session),
plus one explicitly invoked ignored cross-language test. All 143 stage-6 tests
remain; two added receipt regressions cover overwrite/symlink protection and
rejecting an unexpected private response field before writing it. The existing
real-control/importer test additionally checks receipt deserialization, entry
identity, auth revision, mode and absence of upstream/context data.

Nine Python offline tests cover full synthetic HLS audio/video traversal,
MP4 absolute samples, external references/redirect rejection, oversized and early-close incomplete manifest
rejection, count/byte/deadline budgets (including a stalled header parser), URI/grant/session isolation and private
file safety. Run separately; Cargo's default suite never requires Python:

```
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools -p 'test_*.py'
```

An explicitly invoked ignored Rust test launches the same Python probe against
real Rust runtime listeners, playback verifier, encrypted store and Delivery.
Only the existing cfg(test) resolver/socket boundary points to a synthetic
upstream. It exercises the full HLS master/audio/subtitle/TS/WebVTT graph and
progressive MP4 HEAD/start/middle/end samples, validates success and redacted
reports, then shuts down and joins listeners/network/store fixtures. These media
prefixes are protocol fixtures, not decodable movie files.

```
cargo test --locked --lib acceptance_probe_uses_production_listener_for_hls_and_mp4 -- --ignored --nocapture
```

The opt-in test passed for both formats. Cargo.lock is unchanged and default
Cargo tests remain offline without Python, FFmpeg, providers or Mullvad.

## Authorized live results

Configuration: the actual debug daemon, private control `127.0.0.1:18091`, stream
`127.0.0.1:18092`, ephemeral Ed25519 signer/public keys and AES encrypted SQLite
state under an ignored mode-0700 operator directory, default production transport
and streaming bounds. Session leases were 30 minutes (isolated MP4 15 minutes),
initial HLS grants 25 minutes. The signer substitutes for the future API only;
this does not establish API adoption. Development restarts reused the same store,
keys and URLs, but did not preserve active TCP connections.

Fresh `relay-netcheck CONFIG` attested both IPv4 and IPv6 Mullvad exits through
the production connector. Three native Go `vidfast-check` runs queried all four
adapters without fixed priority, using movie TMDB 550, TV TMDB 60625 S1E1, anime TV
TMDB 1429 S1E1. Each selected VidFast 1080p HLS with respectively 7, 9 and 8 ordered
backups. The exact selections imported via production `relayctl`, all HTTP 201.
One additional isolated `vidlink-check -vidlink-only` lookup of the same movie
selected 532p progressive MP4 with two backups. This was a format-specific test,
not a production provider preference; no provider code/registry/ranking changed.
Its production relay HEAD and first/middle/last 64 KiB ranges passed: four
requests, 196,608 bytes received. MP4 was not decoded or played.

The HLS master entries returned 200 after the fixes below. Selected rewritten
movie/TV/anime children returned 200 with respectively 802,506 / 127,122 / 148,746
bytes and 1,393 / 222 / 258 fetch references, including fMP4 MAP initialization.
These exceed the general graph probe's intentional 64-request cap: the graph
probe correctly remains incomplete, rather than certifying all source leaves.
No real AES-128 or independently associated audio/subtitle rendition was accepted.

Using FFmpeg **7.1** from the `imageio-ffmpeg` **0.6.0** PyPI arm64 wheel installed
in ignored local tooling, all three HLS cases independently produced **672 decoded
video frames / 30.030 seconds** and **466,570 bytes mono 8 kHz s16le PCM / 29.16
seconds**, with nonzero samples and successful exit, zero error-output bytes.
Each used the actual Rust streaming listener. FFmpeg ran under a per-process
macOS sandbox denying outbound networking except loopback, with only
pipe/http/tcp/crypto protocols and no source URL or grant in its argv. Master text
was received from the relay, references validated as same-session paths and made
absolute loopback URLs in memory for stdin; no media was saved/spooled. This
establishes decoded content, not title identity, spoken language or full-film
continuity. FFprobe was not used.

The temporary local HTML acceptance page delivered no media; its video element
pointed directly to the production relay. Actual Safari native HLS emitted
loadedmetadata/playing, progressed to 18.25 s, then an explicit `currentTime=600`
seek emitted seeked/playing and progressed through 629.85 s with increasing
rendered-frame counts (744 observed at that point), readyState 4 and no media
error. Its 60-second stop timer paused at 631.30 s. An audio track and a text track
were exposed, but no audible-language or rendered-subtitle assertion was made.
The test-owned tab and local page server were closed afterward. Native player
accessibility exposed an ephemeral grant once; later snapshots were redacted and
the test credentials were disposed after stopping the service.

## Live budget incident and acceptance limits

The experimental late-seek decoder command at movie offset **3600 s** timed out
without a successful decoded-seek result. Its intended byte limiter polled relay
counters **between commands**, not continuously or at the actual receive boundary.
The command consumed much more media than anticipated while seeking, exceeding
the approved **512 MiB** whole-run cap. The last completed counter read for the
final daemon process was **652,041,806 upstream bytes** (about 621.8 MiB); earlier
development daemon processes consumed additional bytes, so this is a lower bound
for the whole stage, not an exact aggregate. The final process did not reset its
counter during that command. The agent stopped FFmpeg and the test daemon when
the overrun was noticed; all further live testing ceased. This was a harness
failure, not a successful bounded measurement or proven relay memory defect.

The between-command limiter is **not delivered as reusable acceptance tooling**.
The committed prefix/graph probe has byte/read/request and absolute wall bounds;
it does not launch a decoder or claim to bound nested FFmpeg/Safari traffic.
Before another live decoder/player run, implement and offline-verify a receive-
boundary aggregate byte gate shared across all nested requests/processes (not
merely a post-command metrics poll), plus a process owner that stops and joins
all clients at the wall deadline. A new bounded run requires fresh approval;
there is no remaining media budget in this approved run.

Consequently: second late decode positions, TV/anime/MP4 Safari checks, longer
sustained playback, live explicit backup reload/resume, active drain/restart
recovery and actual component replacement/outage checks remain **open**. No
playback result was inferred from a stopped/unperformed check. All six original
production boundaries, guarded egress and bounded pull-based streaming remain.

## Demonstrated defects and review corrections

Live selected masters failed `unsupported_manifest` on legacy PROGRAM-ID and
VIDEO-RANGE. Support now validates unsigned PROGRAM-ID only for versions 1–5,
validates VIDEO-RANGE as SDR/HLG/PQ and preserves the attributes through filtering.
Selected media playlists also contained legacy ALLOW-CACHE: YES/NO is retained
only for versions 1–6; duplicates, invalid values and version 7 use are rejected.
Cache hints never relax the relay's private/no-store response policy. No unknown
fetch attribute/tag is silently admitted. Synthetic regressions cover valid
preservation/URI closure, malformed values, overflow, quoting and version order.
See [RFC 8216 section 7](https://www.rfc-editor.org/rfc/rfc8216#section-7) and
[HLS second-edition VIDEO-RANGE](https://datatracker.ietf.org/doc/html/draft-pantos-hls-rfc8216bis-22).

Default FFmpeg requests HTTP partial representations while opening playlists.
The relay's blanket 400 rejection prevented startup; disabling that behavior
proved a 30 s movie decode before the production fix. The relay now ignores
syntactically valid Range on rewritten manifests and bounded keys, returns the
complete validated 200 representation and never sends those ranges/If-Range
upstream. Malformed Range still fails. A real-transport fixture regression covers
GET/HEAD, open/closed/suffix/unsatisfiable client ranges, unchanged whole bodies,
no Content-Range, private/no-store and no upstream Range. Default FFmpeg then
passed all three startup video/audio cases without the workaround.

Greptile's completed initial review of
`b02ed595e467eeb4647a184f64c952380f914b63`, check **116726744417**, reviewed 13
files and found one valid P1 in the acceptance probe: an early-close Content-
Length response could pass with a partial playlist. It now compares received
manifest length to declared length. A regression includes a complete child
reference followed by early EOF and verifies no child is fetched. The nine-test
Python suite and cross-language check pass after the correction. Final-head
follow-up evidence is recorded in the PR/delivery report.

`relay-netcheck CONFIG PRIVATE_SELECTION [selected-variant]` is now an explicit
optional bounded diagnostic of the saved selected HLS root/child through the
same attested production transport. It emits byte counts and static allowlisted
tag/attribute names/counts (unknown buckets replace arbitrary names), never URI,
attribute values, headers, raw manifests or source metadata. It was used to
identify the live compatibility blockers above; it is not a decoder or player.

Receive-boundary shutoff correction: see [opt-in acceptance owner and accounting](acceptance-run.md). The corrected tooling is verified offline; a fresh live run still requires approval.

## Coordinated provider HTTP integration

The earlier empty-provider-HTTP statements above describe the historical run.
Current code/evidence and all open acceptance layers are reconciled in
[the integration matrix](http-integration-acceptance.md). The paired implementation
adds actual HTTP-boundary synthetic coverage, owned provider outage/restart, and
fresh-source orchestration. No fresh live requests have run. Overall acceptance
remains open and the prior authorization remains ended.

Owned decoder/native-player tooling is now prepared and synthetically exercised;
see [the current integration evidence](http-integration-acceptance.md) and
[the complete proposed playback allocation](http-integration-live-plan.md).
No new live traffic is implied. All historical limitations above remain historical,
and overall playback acceptance stays open pending fresh explicit approval.
