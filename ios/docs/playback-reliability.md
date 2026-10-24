# Playback reliability

Local implementation, October 19, 2026. No server deployment or physical-phone
installation is included.

## Changes

- The sibling API serializes by create key or playback ID instead of holding one
  global playback mutex during provider/relay calls. Durable idempotency,
  mutation reconciliation, revocation and bounded admission remain in place.
- General speculation owns one task containing authorization plus an optional
  paused player. Claim removes the entry before awaiting; no completion task
  repopulates a separate ready cache. Obsolete known sessions are revoked.
- Background/profile changes clear speculation. Continue Watching preparation
  tracks profile, coordinates and progress rather than only the show's title ID.
- Structured API failures allow bounded same-key setup/mutation retries.
  Ambiguous create replies retain their key for later reconciliation.
- Failed items and 15 seconds of uninterrupted waiting trigger bounded automatic
  recovery: backup first, fresh sources when the API requires it. Two recovery
  cycles are allowed per player presentation; manual retry resets that budget.
- Recovery coordinates with renewal, preserves the latest acknowledged revision,
  seeks to the current/saved position, and keeps the progress tracker identity.
- Resume seeks have cancellation and an eight-second bound. Reopening waits for
  the preceding final progress writer before creating its new session.
- OSLog category `Playback` records authorization and first-frame elapsed time
  plus recovery attempt count. It does not log signed URLs or provider secrets.

These changes do not pipeline provider probing, change provider cache policy,
introduce relay connection pooling, or establish real-provider startup latency.

## Offline regression checks

Run only when testing is authorized:

```sh
./tools/check-playback-reliability.sh
```

This compiles the production API adapter, request models and preparation/mutation
owners into a local macOS harness. A custom URLProtocol intercepts every HTTP
request; the household server and providers are never contacted. Checks cover
busy setup key reuse, single preparation transfer, late cancellation, backup to
fresh-source fallback, ambiguous mutation reconciliation, renewal revisions and
waiting for final progress saves, and ambiguous creation key retention. This harness is not an iPhone device test.

The sibling API checks are `go vet ./...` and `go test -race -count=1 ./...`.
Regressions cover blocked speculative lookup versus foreground playback,
concurrent identical creation, cancellation and admission cleanup/capacity.

## Disposable simulator fixture

`tools/prepare-simulator-fixture.py /tmp/a-new-fixture-directory` copies the app
sources and Xcode project into a new directory. Only that copy installs an API
URLProtocol fixture and substitutes local synthetic media at the player boundary.
Build the copy for a simulator with `SWIFT_ACTIVE_COMPILATION_CONDITIONS=DEBUG`.
Production source targets do not include the simulator fixture.

Compile `tools/GeneratePlaybackFixture.swift` with `xcrun swiftc
-parse-as-library` and run it with an output `.mp4` path. Copy the generated
`playback-fixture.mp4` into the built `.app` before simulator installation.
Launch with `SIMCTL_CHILD_FS_FIXTURE_FAIL_FIRST=1` to make initial media fail and
exercise backup recovery. The fixture contains one synthetic Supernatural episode
and a second title; neither represents actual provider video. Its API data and
progress are local synthetic state, reset on process relaunch.

A passing synthetic player flow confirms client control/lifecycle behavior. It
does not prove Server deployment, VPN behavior, provider availability, device
buffering, audio/subtitles or an instant start for arbitrary cold titles.

## Verification recorded in this change

- `go vet ./...` and `go test -race -count=1 ./...` passed in the API repository.
- All eight offline client checks passed, including retention of an ambiguous
  creation key across a later retry.
- The production-source simulator build passed with signing disabled.
- A disposable fixture build ran in the iPhone Air iOS 27.0 simulator. UI checks
  displayed advancing synthetic video after Resume, returned to the catalog on
  Close, and resumed near the saved position (about 31 seconds after closing at
  about 30 seconds). No manual retry was used in the passing resume checks.
- The second synthetic title played after its identity was retained correctly by
  the test fixture. The fixture was corrected to decode streamed JSON bodies and
  preserve title identity during simulated recovery; those were fixture errors,
  not evidence about production source availability.
- With initial media deliberately missing, runtime Playback logs recorded an
  automatic recovery attempt followed by the first displayed frame. Both the
  episode and second-title checks exercised this path without a retry tap.

All simulator media was generated locally. No real provider video, deployed
service, VPN routing, or physical device was exercised. No real-world startup
speed claim follows from the synthetic first-frame timings.

## Verified startup preparation follow-up

The existing single-entry preparation now owns a paused-player preroll, bounded
to eight seconds and cancelled on transfer/discard. Readiness observers and the
preroll share that owner; late callbacks cannot restart a claimed player. The
follow-up warmup now owns a cancellable seek near the saved resume position;
claim/discard settles that seek before another owner repositions the player.
No watch-progress update occurs during preparation. Restart and resume share an
authorization preparation key because their API payloads identify the same title
and episode; the foreground request still decides the seek. Presentation IDs
remain distinct. OSLog `Preparation` records preroll completion and whether a
claimed item was ready, without logging signed URLs or profile data. The offline
checks cover exactly-once transfer for both resume and restart.

## Physical iPhone timing diagnostics

Debug builds record bounded local timing events under
`Library/Caches/PlaybackTiming/events.jsonl` (one 1 MiB file plus one rotated
file). A serial utility queue writes these events; no per-frame disk work or
remote upload is added. `PlaybackTiming` uses the phone's monotonic uptime.
Wall timestamps only identify attempts; they never determine durations.

Play, Restart, Continue and Next actions carry a random diagnostic context into
the presentation. The context is excluded from request equality, hashing,
preparation keys and idempotency keys. Continue starts timing before resume
metadata work. An uninstrumented presentation is explicitly labeled
`presentation`, and the analyzer never reports it as tap-to-frame. A new
initial-authorization Try Again gets a separate `retry` trace; automatic
recovery is marked within the original attempt, not presented as another
independent first-frame measurement.

Events distinguish final-progress write barriers, authorization including
transport retries/decoding, prepared-player claim wait, player construction,
resume seeking, actual surface attachment, item readiness, AVKit first-frame
readiness, playing state, and observed advancement. The first-frame milestone is
`AVPlayerViewController.isReadyForDisplay` KVO, not camera proof of on-screen
pixels. Advancement uses the existing 0.5-second observer and is therefore a
coarse upper bound. Each fixed milestone is emitted once per trace.

Speculative authorization/preroll has a separate trace, linked at foreground
claim, so work before Play is not incorrectly added to or subtracted from the
tap latency. A `preparationClaimed` flag indicates whether an AVPlayer was
transferred; its numeric value indicates whether the item was already ready.
Next-episode authorization preparation can transfer a source without a player.
The initial authorization request carries `X-Playback-Trace-ID` consistently
across retries. This change does not deploy backend instrumentation: phone
exports measure the backend round trip, not its provider/relay sub-stages.

Exports contain fixed stage/action labels, random trace IDs, public title/episode
coordinates, timestamps, booleans and numeric measurements. They exclude profile
IDs, title names, error text, playback/grant URLs, source URLs and credentials.
Caching/logging failure never triggers a playback retry.

After building/installing on the paired iPhone Air without uninstalling:

1. Open the app and Supernatural. Press Play promptly. Let video advance for at
   least five seconds, then close playback normally.
2. Open the same title again, wait about ten seconds on its details screen, then
   press Play. Let video advance for five seconds and close playback.
3. Also exercise Continue if diagnosing resume. Normal viewing can update watch
   progress; timing collection itself never issues progress or playback requests.
4. With the paired, unlocked phone reachable, run
   `./tools/collect-device-playback-timings.sh`. It copies only the two known
   diagnostic files and writes JSON plus a Markdown summary in a new private
   temporary folder. An optional first argument selects a local output folder.

A launch marker confirms instrumentation and container extraction independently
of playback. An empty playback summary means no attempt has been captured; do
not present it as device performance acceptance. App data is preserved. Offline
checks: `./tools/check-playback-reliability.sh` and
`python3 tools/check-playback-timing-summary.py`.

## Resume, lifecycle and adaptive playback follow-up

Preparation stamps the final-progress generation after its write barrier settles.
A later progress write invalidates earlier resume authorization; versions are
bounded and eviction cannot recreate the old generation. Warmup remains a
paused, owned player: it seeks to the saved position before preroll, settles the
seek on matching claim, and cancels/joins it on a different target or discard.
Restart explicitly returns a prepared resume player to zero. Foreground startup
skips a redundant seek only when both the preparation and actual player position
match the target. Initial authorization retries keep their idempotency key.

Closing captures the active source, stops/flushes its progress owner, then a
bounded finalizer waits for final progress and revokes the server session. It
never cancels the progress save to make close faster; server leases remain the
fallback if the device is offline or the finalizer limit is reached. Session
expiry recovery uses a new creation key only for a known expired source and
preserves current position and play/pause intent. Automatic recreation is bounded
against loops. Foreground reactivation renews near-expiry grants promptly.

HLS exposes Auto and maximum720p/1080p controls. These change AVPlayer's
preferredMaximumResolution without replacing the item, changing the session or
forcing a rendition that the connection cannot sustain. Auto uses the relay's
verified same-source ladder, bounded by the request's explicit1080p device
ceiling. MP4/single-rendition fallback cannot synthesize missing qualities.
Quality follows the presentation across episode changes like playback speed.

Diagnostics include warmup seek phases, up to24 resolution transitions, quality
ceiling changes and bounded numeric access-log samples. No per-frame disk I/O
or URL export is added. The analyzer preserves transition arrays separately
from initial readiness. Offline regressions cover stale progress invalidation,
close-before-revoke ordering and trace bounds. Actual adaptation, startup timing,
resume, reconnect and title-switch acceptance are measured separately on clients.

The quality menu is shown only when the server reports multiple offered
rendition heights for the registered session. Buttons correspond to those
actual heights. Unknown, legacy and single-rendition sessions keep automatic
playback without offering a quality switch that cannot exist. The numeric
capability list is bounded and validated in both API and client.
