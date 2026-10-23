# One proposed owned playback run — NOT APPROVED

Fresh explicit approval is required before running `playback_operator.py run`.
The original run remains ended: Go reserved 483,330 response-allowance bytes over
five HTTP/DoH requests; Rust used zero. Both original ledgers remain sealed and
are never opened by this tooling. The family-specific Go diagnostic correction
is offline-tested and still live-unverified. No remaining prior allowance exists.

## Sources and limits

All four native adapters (VidCore, VidLink, VidSrc, VidFast) participate through
POST `/internal/v1/resolve` and the shared production engine. No fixed winner,
isolated-provider CLI, replacement title, direct decoder URL or protocol fallback.
The actual Selection response goes unchanged to production relay control.

| Case | Exact lookup | Required initial format | Temporal seeks |
| --- | --- | --- | --- |
| Movie | movie TMDB 550 | HLS | 600 s, 3600 s |
| TV | TV TMDB 60625 S1E1 | HLS | 600 s, 1200 s |
| Anime | TV TMDB 1429 S1E1, MyAnimeList 16498 | HLS | 600 s, 1200 s |
| Progressive | movie TMDB 550 | MP4 | 600 s, 3600 s |

Each lookup advertises only its required format, avc1/mp4a codecs and maximum
720-line video. Unknown language stays unknown. Normal validated source redirects,
HLS renditions, maps, keys and subtitles for these titles are included. Missing
format/source/backup/track or a shorter timeline is recorded as unavailable or
incomplete; it does not authorize a replacement title or extra resolution attempt.

One allocation, created once **before either attestation**:

| Scope | Approved maximum proposed |
| --- | --- |
| Whole run, including attestations and restarts | 1800 seconds |
| Rust reserved receive allowance | 1536 MiB = 1,610,612,736 bytes |
| Rust aggregate upstream HTTP exchanges, including DoH/retries/redirects | 8192 |
| Go aggregate response allowances, including provider/probe/DoH | 384 MiB = 402,653,184 bytes |
| Go aggregate HTTP/DoH requests | 512 |
| Additional transport/kernel/VPN overhead headroom | 256 MiB; not a measured wire counter |

Go reserves complete body/header/read-ahead allowances before requests, including
failed work, without refunds. Rust uses the corrected receive-boundary ledger,
including per-connection kernel headroom and reservations before socket reads.
Neither is an exact ISP/wire counter. TCP/IP/VPN framing, packets in flight and
retransmissions remain outside the accounting claim; see acceptance-run.md and
providers/docs/http-service.md. The extra headroom is an explicit uncertainty
allowance, not an independently enforced packet cap. If an exact wire-byte cap is
required this run cannot supply that evidence. The owner stops/joins clients on
either ledger ending or the original deadline. Never recreate/reset/refund or
replenish either ledger to continue this run, including after an early failure.

## Concrete tooling and execution

Offline preparation uses `tools/playback_operator.py prepare`, the actual built
Go providers/netcheck and Rust relay/netcheck binaries, locally compiled
`tools/playback_player.swift`, and the existing local FFmpeg 7.1 binary. No new
package download is needed. Preparation generates a mode-0700 directory outside
Git, private mode-0600 configuration/keys, exact executable/tool SHA-256 hashes,
lookup files, and signer inputs. It creates **no allowance** and contacts no source.
The private prepared receipt records both exact Git heads. Changed tools/binaries
are rejected at run time. Ports 18090/18091/18092 must be free; never displace a
process. No catalog/API/client/reference code or deployment is involved.

After approval only, `run --opt-in --directory PRIVATE_DIRECTORY` does the following:

1. Exclusively marks that directory single-use and creates both ledgers once with
   the same absolute deadline. Run Go `providers-netcheck` through its production
   bound client and tunnel-bound DoH, then Rust `relay-netcheck` through its
   production guarded transport with the Rust ledger. Require positive IPv4 and
   IPv6 Mullvad assertions from **both** before any provider resolution/media.
   Failure ends this allocation, seals both ledgers and removes ephemeral inputs.
   Safe fixed failure categories/per-family booleans are retained; no raw output.
2. The corrected acceptance owner starts the actual relay and **one** manifest
   client, `playback_acceptance.py`, representing future API orchestration. Its
   four cases run sequentially, stop on failure, and use the original aggregate
   ledgers. Each case has 480 seconds, 64 loopback provider/control requests;
   each decoder process has at most 45 seconds. Native player operations have
   their requested playback duration plus 40 seconds startup/seek margin. All
   nested media requests share the aggregate Rust 8192-exchange/byte gate; there
   is no additional decoder/player media allowance or polling-based byte limiter.
3. Each case owns a fresh Go daemon, reattesting Go before opening that same
   allowance on **every** start. It calls the real endpoint, imports its exact
   Selection via production `/internal/v1/sessions`, signs a short-lived grant
   in test-owned tooling, and uses only authorized production `/stream/*` routes.
   Native/decoder entry URLs are supplied privately, never in argv/logs/UI.
4. FFmpeg independently decodes **10 seconds video and 10 seconds audio** at the
   start of each case, discarding decoded frames/PCM after numeric counting.
   It uses stdin for a private single-entry input and only pipe/http/tcp/crypto
   protocols. No source download, media spooling, transcoding, direct-source
   fallback or downloaded JS/WASM execution. Nonzero PCM proves no language.
5. A dedicated visible **AVPlayer/AVPlayerView** subprocess plays **120 seconds
   from zero**, then seeks the same item to both table positions and plays
   **15 seconds after each seek**. Require completed temporal seek, timeline
   advancement, decoded pixel-buffer progression, view ready for display and
   maximum observed inter-frame gap <=5 seconds. Record first-frame latency and
   actual positions/frame counts. These are native local-player checks, not
   physical-device/Safari or full-film continuity claims.
6. For available audio/subtitles, preserve the original master/rendition groups.
   Record exposed native options; the native default audio is decoded, with no
   inferred spoken language. Select the first available legible option. Source
   cues render in this test player's visible custom subtitle overlay. Verify white
   glyph rasterization against its black background **in memory**, then discard
   the bitmap; retain numeric cue/render counts only. No cue text or capture is
   saved. This supports this custom renderer, not Safari/iOS subtitle rendering.
   No delivered cue means rendered subtitles remain open, even if a track exists.
7. In **each usable case**, keep the native player/item running, stop/join only
   Go and fetch the existing authorized entry (GET HLS/HEAD MP4) plus observe
   **10 seconds** continued frames/time, restart Go with its
   original config/ledger and fresh attestation, fetch the existing entry again
   and observe another **10 seconds**
   on that same item. No provider call is needed per relay media resource.
8. If an ordered backup exists, activate it explicitly via production control,
   require increasing generation and `resume_required`, replace the player item
   with the new authorized entry, restore its measured media time, and play
   **10 seconds**. Record the activated format if different; never splice sources.
   Then perform exactly one uncached HTTP resolution (`lookup.refresh=true`),
   import through `/source`, require a newer generation/resume_required, reload
   at measured time and play **10 seconds**. Missing backup is unavailable.
9. In **each usable case**, the owner signals its exact relay child for drain,
   waits for exit, and restarts on the same encrypted store/keys/Rust ledger
   (at most four restarts total, one numbered request per case).
   The player remains owned; the orchestrator waits bounded readiness, verifies
   persisted entry/auth/source generation, explicitly reloads that entry at the
   last measured media time in a freshly launched owned native client and plays
   **10 seconds**. The earlier native client is stopped/joined first, even if it
   exited on a terminal media error during drain. Active TCP connections do
   not survive restart. This proves only this bounded reload/resume strategy.

The native and FFmpeg subprocesses inherit the manifest client's process group;
no browser or user application is attached. The native helper has its own 600 s
monotonic ceiling. Child network sandboxing allows loopback only; the actual
source/DoH traffic remains exclusively in existing Go/Rust guarded transports.
The player can prebuffer; native buffer hints are advisory and never substitute
for the receive-boundary gate. System-provided AVFoundation codec services are
platform infrastructure, not independently launched source clients.

## Stop, cleanup and evidence

At a ledger end, deadline, signal or failure, the owner seals Rust, signals/kills
owned process groups, joins direct children and seals the original Go ledger
**after process cleanup**, even if orchestration was killed. Native/FFmpeg are
also stopped/joined by their direct owner on normal/error exits. Normal cases
revoke their sessions. If a hard stop prevents revocation, stop the isolated
relay and discard its private session DB. Cleanup may finish after transfer stops.
No daemonized/unowned client is permitted. OS/owner SIGKILL or host failure cannot
promise final cleanup; those are failure outcomes, never resumable allocations.

The operator removes only its named ephemeral configuration, signer/secrets,
lookup/report inputs and session DB directory, preserving unrelated files. Retain
only stopped ledger metadata, a single-use marker and a redacted numeric outcome
outside Git. Report exact tested heads/hashes, each attestation, reserved bytes/
requests, stop reason, joined-process counts, per-case completed checks and gaps.
Do not equate bytes with decoding, ranges with seeks, cues with rendered glyphs,
nonzero PCM with language, or synthetic tests with live acceptance.

No VPN/firewall change, tunnel disruption, Server/SSH/deployment, physical device,
existing browser, added title, allowance extension or PR merge is included.
