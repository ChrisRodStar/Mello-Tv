# iOS playback experience review

Reviewed from local sources on October 17, 2026. Working branch:
`ios/playback-experience`.

The initial review is a source audit, not measured playback performance. No build,
test, live provider request, device installation or remote-host operation was run
for that initial review. Subsequent implementation and validation are recorded below.
Existing app sources, Xcode project and README changes were already present and
uncommitted when the branch was created. This review preserves them.

## Current system

The native app has already moved into this independent repository and uses the
Go `/api/v1` adapter. It is no longer a README-only scaffold. The adapter is
implemented; its README's deployed-service claims were not checked against Server.

| Responsibility | Current owner and path |
| --- | --- |
| Profiles and onboarding | `ProfileStore`, public Go API household state; active profile selection is local per origin |
| Browse and recommendations | `CatalogStore` combines saved browse/recommendation envelopes; catalog owns SQLite/TMDB refreshes and API retains readable snapshots |
| Search and details | Public API cached records; details/seasons use bounded polling while catalog metadata refreshes |
| Favorites, library, history | API-owned household/profile data, adapted by `FamilyStreamAPI` and `LibraryStore` |
| Playback setup | `PreparedPlaybackService` sends a profile-scoped POST with title IDs, episode coordinates, device capabilities and an idempotency key |
| Source selection | Go providers resolve, measure eligibility and rank candidates through guarded Mullvad networking |
| Media delivery | Go API registers the complete private selection with Rust relay and returns a signed same-origin `/stream/` URL |
| Playback and progress | AVPlayer consumes relay media directly; `PlaybackProgressTracker` serializes cumulative measured-time updates under the API playback ID |

Known-title playback does not synchronously query catalog or TMDB. Public API
traffic and relay traffic share the client origin, but media bytes bypass the API
process. Providers and relay own upstream access. The phone must never receive
raw provider URLs, provider credentials or private service tokens.

The app uses the same hardcoded private HTTPS origin for simulator and device.
It disables API cookies, rejects API redirects, validates playback identity and
same-origin relay URLs, and advertises HLS/MP4 with AVC/AAC. Preserve these
boundaries during player work. The API's persisted browse snapshot is distinct
from on-device offline support; the iOS stores primarily retain data in memory.

## Where the wait comes from

The current path is:

1. The viewer presses Play. TV selection may first depend on loaded episode data.
2. The app presents `PlayerView`; its task starts the playback POST.
3. API persists setup identity and invokes the providers service.
4. Provider adapters run concurrently. The coordinator collects their outcomes
   before starting media probes. Collection ends after all adapters respond or
   the default 18-second collection deadline.
5. The probe pool measures all collected candidates before ranking. Defaults are
   four concurrent probes, six seconds per source and a 25-second overall search
   window. These bounds overlap; they are not additive expected timings.
6. API registers the selection with relay and returns the signed entry URL.
7. AVPlayer loads the manifest/file, chooses tracks, buffers and starts playback.
   Resume currently happens later when the progress tracker sees a finite duration.

The provider result cache lasts up to 20 seconds, shortened for source expiry.
It caches adapter results, not completed playback readiness: cache hits still
participate in collection and media probing. The app's `preload` method is
deliberately empty, so opening details does not hide the lookup cost by creating
sessions or probing sources speculatively.

This makes a slow provider a plausible source of unnecessary wait before probing
even when another provider answered quickly. It does not establish that every
Play takes 18 or 25 seconds. Actual device and server stage timings are needed.

Source anchors: sibling providers `internal/search/coordinator.go` (`run`),
`internal/search/deadline.go`, `internal/probe/pool.go`,
`internal/cache/sources.go`; sibling API `internal/api/playback.go`
(`createPlayback`, `resolve`).

## Findings and proposed fixes

| Priority | Finding in current code | Proposed change |
| --- | --- | --- |
| First | `PlayerView.startPlayback` clears loading and sets playing immediately after `play()`, before playback actually starts | Drive source lookup, initial buffering, playing, paused, rebuffering and failure from distinct states and observed AVPlayer status |
| First | Source lookup shows a generic spinner with no close control | Show title/episode context, truthful “Finding a playable source” then “Preparing video” messaging, and an always-available close action; cancel the owned request on dismissal |
| First | Custom chrome disables native controls and offers no audio/subtitle chooser | Choose native AVKit controls or a deliberately minimal custom design with real track selection; retain AVPlayer/relay transport |
| First | API/setup failures and retries discard the idempotency key | Retain the key for an ambiguous or retryable setup failure; an explicit new session is a separate action. Preserve structured public error codes so the UI can distinguish busy, expired, unavailable and no-source cases |
| Next | Resume seek is initiated by the progress tracker after playback can already start | Move initial resume orchestration into the playback owner; wait for readiness and complete the initial seek before starting playback. Keep measured progress accounting separate |
| Next | Renewal ignores `reload_required` and `source_generation`; reloads whenever URL changes | Decode the public contract fields and honor reload instructions; preserve intended play/pause state through buffering and renewal, retain playback identity and rebind item observers |
| Next | `onNextEpisode` exists, but no current `PlayerView` call site supplies it | Resolve next episode coordinates using saved metadata, explicitly handle season boundaries/end of show, and connect the action to the presentation owner |
| Next | Raw custom scrubber lacks accessibility adjustment; some buttons lack labels/44-point targets; double-tap uses `UIScreen.main` | Use native controls or an accessible slider/buttons; use the actual video container width for location-based gestures; keep paused and VoiceOver controls visible |
| Next | API exposes backup/refresh, but the client only starts setup again after a source failure | Add bounded, explicit source recovery using the same playback ID and revision/idempotency semantics; tell the viewer when reloading. Do not loop across providers automatically |
| Separate providers work | Collection and probing are sequential phases; final selection waits for all probing | Start bounded probes as adapter outcomes arrive. First preserve existing ranking/output semantics; then evaluate an explicit early-selection policy against measured startup and source quality |

Do not replace the source coordinator with a fixed favorite provider or accept an
unprobed candidate. An early-selection policy changes ranking and backup behavior:
define its eligibility threshold, deadline, cancellation/join behavior and ordered
backup result before implementation. Background results cannot silently change an
already registered relay selection. That work belongs in the providers repository
and its own branch, after reading its local instructions.

## Player direction

Compact player chrome: the bottom group uses a 32-point layout row for the
seek bar followed directly by 44-point action targets, with an 8-point bottom
inset. The seek target retains its 44-point height by extending upward into the
video; it does not overlap the action targets. The native UISlider now draws a
3-point track and a 12-point round thumb, with immediate native tracking and the
existing seek completion and accessibility actions. Smaller inline icon/text
actions share the available width evenly; shorter labels remain available on
narrow layouts. Brightness/volume gesture handling is unchanged. Design guide
lines are not rendered in the app. No automated tests or playback probes ran.

Episode-spacing revision: each card is one natural-height group with a 16:9
preview, a 12-point gap to its single-line title area, an 8-point gap to runtime,
and a 6-point gap to the four-line synopsis. There is no flexible spacer inside
the card. The complete shelf is centered in the area below the header with
16-point minimum vertical padding; images and copy move together. Titles allow
two lines at accessibility text sizes. The approved annotated guide is reflected
in the compact title row, without displaying guide lines in the app. Vertical
overflow remains available for larger text. The annotated image is a layout
guide, not an on-device capture. The physical-device build passed; visual
acceptance remains manual.

Thumbnail progress bars
now cover previously watched episodes through per-profile exact `/resume` reads
for visible cards. Replies are checked against TV/title/season/episode identity
and finite position/duration. Bars use actual saved position, while the current
episode uses live player progress. Missing or unavailable records never fabricate
completion. Optional progress errors do not block episode selection. Existing
API routes are reused without full-library hydration. The device build passed;
no automated tests or live playback/progress probes ran.

Episode-shelf revision: the supplied Netflix reference informed a horizontal
episode browser with large 16:9 previews, a season menu, and a 48-point close
target. Cards reserve one title line (two at accessibility sizes) and four synopsis lines, with runtime in
a separate secondary row. The current episode has a Playing badge, red border
and actual current playback progress; selecting it closes the browser without
creating another session. Other whole-card targets select playback using the
existing dismissal/handoff flow. The shelf opens at the current episode and
aligns cards as scrolling settles. Vertical overflow remains available for short
landscape displays and larger text. Unsupported downloads are omitted. The device
build passed; no automated tests or playback probes ran. Spacing and interaction
acceptance remain for the user's physical-device feedback.

Landscape and compact-menu revision: the full-screen presentation owns a per-window
landscape lease, allowing both landscape directions. A UIApplication delegate
returns the window's active mask, and UIWindowScene geometry requests update the
orientation on entry/release. The lease survives episode changes and episode-sheet
presentation; closing restores the previous permitted app orientation. The bridge
uses the actual presenting window and public rotation APIs. Speed and audio/subtitle
menus now render over the video instead of adapting into full-screen landscape
sheets. Speed is a short horizontal chooser; tracks use two independently scrollable
columns. Buttons retain explicit minimum hit areas. A close button and outside tap
dismiss the overlays. Episodes retain the large sheet. The physical-device build
passed; no automated tests or orientation/playback probes ran. Physical rotation
and menu appearance require the user's device feedback.

Touch-target revision after full-day use: explicit rectangular hit shapes now
cover padded player controls, the complete bottom action labels and track rows.
Small controls in search, title details, profile switching, onboarding and
featured pagination have minimum touch areas inside their button labels. Shared
card-button styling covers transparent label space. Synopsis expansion is now a
sibling of episode playback instead of a nested button. Continue-watching artwork
and title are separate playback buttons with the info button as a sibling;
decorative gradients/progress overlays do not intercept touches. The visual icon
sizes and approved swipe recognizers are retained. The device build passed and
devicectl confirmed installation on the paired iPhone Air after the user reiterated
their standing instruction to build/install all completed iOS changes. No tests
ran. Actual touch responsiveness remains for the user's device feedback.

Next-episode preparation revision: the viewer authorized resolving the next
episode early using the existing public API. The full-screen presentation owns
one preparation, triggered while TV playback is running with roughly 120 seconds
of viewing time remaining, adjusted for playback speed. It starts only when the
current grant has more than 180 seconds left, keeping clear of the normal renewal
window. Metadata and source tasks are shared with manual Next Episode; a matching
selection transfers ownership before the current episode is torn down. A prepared
URL must retain more than 90 seconds of validity. Expired preparations receive a
fresh setup key; failed/ambiguous preparation retries preserve the original key.
Backgrounding, dismissal and selecting another target cancel unused work. Known
unused sources receive at most three revocation attempts. An unknown session ID
after cancellation or failed revocation remains bounded by its server lease.
There is one preparation attempt per episode per presentation, without automatic
retry loops. Opening details still does no provider work. The relay/API/provider
contracts are unchanged; the API client adds the existing DELETE playback route.
No next-item AVPlayer buffering is performed. This source revision has not been
built, tested or installed, and no live provider or device requests were run by
the agent. The earlier successful build/install covers the flat player revision.

Screenshot-led presentation revision: the viewer approved the native swipe feel
and requested a flatter Netflix-style player matching Family Stream's aesthetic.
The title/close row and central play/pause control now have no circular backing;
a single subtle scrim replaces the layered gradient. The timeline pairs the red
scrubber with remaining time, above a responsive bottom action row for speed,
episodes, audio/subtitles and Next Episode. Speed (0.5–2×) persists across pauses,
grant reloads and episode changes. An episode picker uses cancellable metadata
loads and explicit retry states. Next Episode finds the next available coordinate
within a season or the first episode of the next available season. Each episode
gets its own player/progress lifetime; sheet dismissal precedes replacement.
Control lock disables seek and adjustment gestures; tapping reveals Unlock.
The approved native touch handling and device-write scheduling are retained.
Visible ten-second skip buttons stay hidden. Clip, ratings and reporting have no
implemented contract and are omitted. This revision has not been built, tested
or installed, respecting the user's request against testing. The earlier device
installation covers the preceding gesture revision only.

Preceding gesture revision, subsequently built and installed at the user's request:
`PlayerTouchSurface` now owns native pan/single-tap/double-tap recognizers on a
transparent input view above AVKit. Taps wait for pan failure; the pan does not
wait for taps. Top/bottom controls, center transport and VoiceOver retain their
exclusions. The adjustment range uses the larger of 360 layout points or 85% of
the input surface height, independently of the pill's visual height. This makes
short swipes less sensitive. Incremental pan deltas preserve immediate reversal
at either limit. HUD state updates directly; device-setting writes retain only
the latest value and apply at display cadence. Pending writes are flushed on
gesture end and cancelled on background/dismissal, with brightness restoration.
The viewer confirmed that this gesture revision feels smooth on the phone.
No automated tests or profiling were run; that feedback establishes the gesture
experience, not provider playback, subtitle, renewal or deployment acceptance.

Keep the existing browse/detail visual language. Concentrate the redesign on the
transition into playback and the playback controls.

Selected by the user: custom minimal controls styled to match the app. Use a
compact title/close row, a central play/pause button and a single bottom timeline
with tracks/options. Avoid permanent large overlays. Keep controls visible while
paused, scrubbing or using VoiceOver, and show a small buffering indicator over
the existing video during later stalls.
Audio/subtitle controls must reflect actual available media selections, not
provider language guesses. PiP and AirPlay need explicit lifecycle/device checks
before being advertised as supported.

Apple documents the relevant distinction between paused, waiting and actually
playing in [AVPlayer.TimeControlStatus](https://developer.apple.com/documentation/avfoundation/avplayer/timecontrolstatus-swift.enum).
Keep automatic stall minimization as the baseline. Disabling it or forcing an
immediate start requires a measured tradeoff between startup time and rebuffering.

The custom controls are implemented in `PlayerView.swift`: a single play/pause
button, a compact header, a standard accessible slider using the app's red
accent, and an audio/subtitle sheet backed by AVFoundation media selection groups.
Preparation distinguishes source lookup from AVPlayer buffering; the close button
remains available during preparation and failure. Playing state comes from
AVPlayer observation instead of the call to `play()`. Auto-hide is suppressed
while paused, buffering, scrubbing, picking tracks or using VoiceOver. Gesture
coordinates use the actual view width, and owned observers/tasks are cleaned up.

This first implementation does not change provider scheduling, initial resume
ownership, retry key semantics, next-episode wiring or API reload fields. The
findings above retain the pre-change audit; these items remain follow-up work.
Track availability/selection in code is not proof of rendered subtitles or a
particular audio language on the phone.

The viewer subsequently requested hidden ten-second skip controls and vertical
brightness/volume gestures. Visible skip buttons are removed; double-tap seeking
and named VoiceOver seek actions remain. A vertical swipe starting on the left
adjusts brightness on the player's actual window screen; the right adjusts system
media volume through the attached MPVolumeView slider. The indicator is a slim
rounded vertical pill with a continuous bottom-up white fill. It has
no icon, box or text. The fill tracks drag samples directly without a spring or
interpolation delay; only visibility fades over 100 ms. Drag recognition begins
at 6 points instead of 18. Finger travel maps directly to filled height, using
incremental movement so reversing at either limit responds immediately. Slight
diagonal movement is tolerated, and recognition does not jump the initial level.
High-frequency indicator state is isolated from the player view, and unchanged
AVKit players are no longer reassigned during updates. Reduce Motion disables
the visibility fade. Gestures lock to their starting side,
exclude the central control and top/bottom bars, and are disabled under VoiceOver.
Brightness restores on dismissal/backgrounding. Volume keeps the chosen level.
The physical-device build passed; real volume-route and gesture acceptance remain
for the user's iPhone testing.

Reference check for this revision: [Netflix's brightness guide](https://help.netflix.com/en/node/114863)
documents up/down adjustment on a vertical indicator. Published Netflix player
screenshots show a narrow edge slider, rather than an iOS Control Center-sized
pill. The custom bar omits iconography as explicitly requested by the viewer.
These references do not establish Netflix's internal timing or implementation.
The subsequent visual reference is the [iPhone volume HUD](https://www.macrumors.com/2019/06/03/apple-ios-13-volume-indicator/).
The viewer requested continuous progress-like filling rather than the very thin
bar from the preceding revision. The revised pill is 20 by 140 layout points;
these dimensions do not represent adjustment steps. Its level remains continuous.

## Delivery sequence

1. **Startup state and measurement.** Add an owned playback state model, immediate
   close/cancellation, separate lookup/buffering messages, and monotonic timings
   for setup request, response, item readiness and first rendered video frame.
   Playing status is not itself first-frame proof. Log only stage durations and
   redacted local correlation IDs, never signed URLs or source payloads. Server
   stage timings must separately identify lookup, probes and relay registration.
2. **Player presentation.** Implement the selected controls direction with
   readiness-driven resume, accessibility and clean error/retry behavior. Preserve
   grant renewal, observer teardown and measured progress across replacement.
3. **Recovery and episodes.** Connect next-episode behavior, structured API errors,
   contract reload fields and bounded source recovery.
4. **Measured source speed work.** Use a separate providers change to overlap
   lookup/probes; compare retained quality, backup behavior and cancellation before
   considering earlier selection. Avoid speculative title-details probes.

Other app findings to address separately: title-detail Play can be visible but do
nothing before TV episodes load; selected-season tasks can race; a completed
season's “Next Episode” selection can fall back to the first episode; profile
switches need stale-result guards in library loading; history hydration performs
serial title fetches. These affect responsiveness or correctness but are not proof
of the source lookup delay.

## Validation to request when implementation is selected

The workspace requires an explicit request before running builds, tests,
benchmarks or deployment commands. The user subsequently requested building and
installing the finished changes on the paired physical iPhone Air for manual testing.

Implementation validation: the Debug physical-device build passed with Xcode
27.1. `devicectl` confirmed installation on the connected iOS device under
the existing `com.streamtv.familystream` bundle ID, without uninstalling the app.
Build log: `/tmp/family-stream-playback-iphone-air-build.log`; derived data:
`/tmp/family-stream-playback-iphone-air-build`. There was an unrelated existing
unused-variable warning in `ProfileEditorView.swift` and the standard skipped
App Intents metadata warning. No automated tests, live provider requests or
remote operations were performed. Phone playback and visual acceptance remain
for the user's manual testing; no startup speed improvement has been measured.

Use offline fixtures for cancellation/retry ownership, ambiguous setup replies,
resume-before-play, stale callbacks, renewal replacement and structured errors.
Build the iOS target after source changes when authorized. Use approved synthetic
local streams for initial playback state checks where possible.

Physical-device acceptance must separately cover movie/episode startup,
tap-to-first-frame timing, pause/seek/resume, real audio/subtitle selection,
buffering, retry, episode boundaries, grant renewal, profile changes and
background/foreground behavior. Report actual startup distributions across cold
and warm lookups, without inventing a speed target before a baseline exists.
Live provider traffic requires a fresh bounded allocation under the local
fail-closed Mullvad rules. No remote deployment is required to develop these changes.
