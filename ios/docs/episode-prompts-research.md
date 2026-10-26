# Skip Intro and next-episode prompts

Research and implementation review: October 26, 2026.

## Intended behavior

The user chose Netflix-style autoplay. A single compact **Play next** button provides an animated fill during the autoplay countdown. Episode metadata and the extra Keep watching action were removed at the user's request. The card appears in the final 60 seconds of playback, adjusted for playback speed, or at a validated final credit interval. Its ten-second countdown arms during the final ten playback seconds after genuine forward playback. Automatic handoff waits for AVPlayer's natural item-end notification, so a community credit marker cannot cut off the story or a post-credit scene. Tapping Play next may advance immediately.

Pause and buffering freeze the countdown. Backgrounding, seeking, and scrubbing reset it; natural forward playback must re-arm it. Locked controls, another player panel, and brightness/volume adjustments hide the prompt and suspend its timer. The prompt state retains its cancellation behavior for lifecycle safety. No successor means no automatic action. Season rollover uses the existing episode resolver rather than assuming that episode numbers are contiguous.

Skip Intro or Skip Recap is a manual action shown only while the playhead lies inside the corresponding accepted interval. It uses the serialized seek queue without revealing the full playback controls. There is no hard-coded intro duration or automatic intro skip.

## Existing implementation audit

The five provider adapters currently return stream, subtitle, quality, and transport metadata, but none exposes reliable intro/credit markers. Catalog episode runtime is a rounded metadata value, not an intro boundary. The actual finite `AVPlayerItem.duration` is the timeline authority.

The app already prepares a successor near the end and transfers its authorization once. The new flow reuses that ownership, progress flushing, cancellation, and episode resolver. A cancelled preparation can be attempted again after foregrounding. Item identity and episode keys reject late callbacks after replacement.

## Sources investigated

| Source | Useful properties | Decision |
|---|---|---|
| [TheIntroDB v4 client documentation](https://theintrodb.github.io/theintrodb-npm/getting-started.html) | Public reads; TMDB/IMDb/TVDB identity; episode order; duration-selected release groups; multiple intro/credit intervals; optional confidence and submission counts | Implement a direct iOS reader with validation and a bounded memory cache |
| [IntroDB.app API](https://introdb.app/docs/api) | Public IMDb show/season/episode lookup; intro, recap, outro, and post-credit records; explicit confirmed-absence flags | Fallback for missing opening/credit markers only when a final outro end closely matches the measured playback duration |
| [AniSkip official API](https://api.aniskip.com/api-docs) | Anime openings/endings and episode-length filtering; MAL relation rules | Deferred: the current TMDB season/episode plus title-level MAL ID does not establish the exact anime episode mapping, especially later seasons/cours |
| [Jellyfin media segments](https://jellyfin.org/docs/general/server/metadata/media-segments/) and [Intro Skipper analysis](https://github.com/intro-skipper/intro-skipper/wiki/Settings-%E2%80%90-Analysis) | Typed segments, named chapter analysis, recurring-audio detection across local files | Useful architecture references; offline library analysis is not a suitable dependency for this provider-streaming path |
| [Apple AVPlayer timing](https://developer.apple.com/documentation/avfoundation/avplayer/addperiodictimeobserver(forinterval:queue:using:)) and [AVPlayerItem](https://developer.apple.com/documentation/avfoundation/avplayeritem) | Periodic observations include time jumps; duration may be indefinite; natural completion is item-specific | Use actual duration, distinguish seeks from natural progress, and guard item ownership |

TheIntroDB's [published terms](https://theintrodb.org/docs/terms) favor direct end-user client access and restrict multi-user server aggregation. This implementation makes direct client reads and does not add a shared server proxy, exported catalog records, or a persistent timestamp database. The request contains the show's TMDB ID, season, episode, measured duration, and matching options. It contains no household/profile identifier or provider media URL. This service receives the device's network request and public title coordinates. The [alternative IntroDB.app terms](https://introdb.app/docs/terms) explicitly permit reasonable player integrations; its timestamp data is not assumed to be open-licensed.

## Validation and bounds

The original Swift reader uses the public raw REST schema, not the JavaScript package's normalized `kind: media` wrapper. It verifies TV type and exact returned season/episode, rejects version/episode-list responses, and requires a known release group whose duration matches playback within `max(2 seconds, min(8 seconds, 0.5% of duration))`.

Accepted intervals require finite integer millisecond boundaries, confidence between 0.5 and 1, and at least two submissions. The confidence cutoff is a product heuristic, not a probability or accuracy guarantee. Intros must be 3–300 seconds long, begin before the earlier of 20 minutes or the episode midpoint, and finish at least 30 seconds before the episode end. Only a final credit interval within the final quarter and final 15 minutes is retained. Credit data may suggest when to show the card; it cannot cause an early automatic transition.

Lookups are asynchronous and do not gate playback startup. Network requests have fixed timeouts, a 64 KiB body bound, no cookies, and no redirects. Each lookup makes at most one primary request and one fallback request (up to 16 seconds of resource timeout in total). The memory cache has at most 32 entries and four in-flight lookups, coalesces equivalent requests, retains successful marker sets for ten minutes, and briefly caches failures/empty results for 30 seconds. Different measured durations use separate cache entries. Invalid or missing timing data yields no opening-skip button; ordinary playback and next-episode behavior continue.

## Live research observations

- Raw TheIntroDB v4 returned a supported Breaking Bad S1E1 intro at 228.566–246.538 seconds for a 3,500-second release group, confidence about 0.566 and six submissions. The production Swift client also accepted this exact duration-matched response. Its lower-support credits were omitted by validation.
- A Stranger Things S1E1 response had no usable intro and contained zero-length records; those must not create controls.
- The Office S2E1 response contained one low-confidence submission for a 1,922-second cut. Both support and release-duration checks matter; a shorter cut must not inherit those timestamps.
- AniSkip's first Attack on Titan episode returned different opening/ending reference durations near 1,442–1,447 seconds. Its relation rules also map later absolute episodes to separate seasonal MAL IDs. This is evidence for explicit episode mapping, not permission to guess it.

These are coverage samples, not proof that every show or release has accurate markers. Matching runtime reduces edition errors but cannot eliminate community-data mistakes. Missing or rejected markers intentionally suppress Skip Intro.

## Verification

`tools/check-episode-prompts.sh` is offline by default. It covers the raw API fixture, schema/identity/edition failures, malformed and low-support ranges, final-credit handling, request identity, coalescing/cache, rate limiting, redirect/content/size failures, countdown state, cancellation, seeks, and background behavior. Its optional `--live` flag performs one public duration-matched timing lookup. `--live-supernatural` exercises the production fallback against the measured S1E15 duration.

`tools/check-playback-reliability.sh` continues to cover preparation handoff, seek accumulation, quality/recovery operations, progress finalization, and cancellation. Device build/install, simulator visual checks, and actual physical-device playback are separate evidence layers; none is implied by a passing model test.


### Completed verification evidence

- Offline marker/state/resolver checks passed, including out-of-order episodes, season rollover past an empty season, and the final episode.
- Existing playback reliability checks passed.
- A live call using the production Swift reader accepted the duration-matched Breaking Bad intro listed above.
- A disposable, network-intercepted simulator build displayed Skip Intro, then the original next-episode countdown and both actions. PlaybackTiming events confirmed `nextEpisode` attached a player and produced a first displayed frame for episode 2 after episode 1 ended. The successor lookup then returned no further episode.
- Simulator screenshots were inspected at `/tmp/mello-episode-prompts-intro-verified.png` and `/tmp/mello-episode-prompts-up-next-fixed.png` (the latter shows the countdown).
- This integration check uncovered a stale captured environment phase in playback callbacks. The production view now mirrors phase changes into live state for callback/timer guards. Successor and definitive no-successor results are cached per player item. Failed or background-cancelled metadata lookups retry after 5 and 15 seconds, with a maximum of three attempts; task tokens reject stale cancellation callbacks.
- The simulator's media and intro intervals are synthetic fixtures. They do not establish timestamp accuracy for every real-world release or physical-iPhone feature acceptance.

### Supernatural missing-prompt investigation

The physical phone history showed S1E15 stopped with about 37 seconds remaining and S1E18 with about 45 seconds remaining. Both were earlier than the original 30-second fallback card. The fallback now appears in the final minute; the final-ten-second countdown and natural-end requirement remain unchanged. An additional defect allowed an initial metadata failure or background cancellation to permanently prevent Up Next for that item. The bounded retry policy above addresses it.

Checked Supernatural records have recaps rather than published intro intervals. Null intro with `no_intro: false` means unknown coverage, not a confirmed intro-free episode. The player must label a recap as **Skip Recap**, never invent or relabel an intro.

The direct IntroDB.app fallback uses the existing catalog IMDb identifier and exact season/episode identity. A valid final outro ending within the same 2–8-second duration tolerance is required before accepting any fallback interval. This is an edition-match heuristic, not proof of timestamp correctness. The fallback requires confidence at least 0.95 and at least one submission; this deliberately permits the single-submission Supernatural recaps as optional manual actions. It does not relax the primary source thresholds. Valid primary opening/credit markers take precedence. Missing IMDb identity, runtime mismatch, missing outro anchor, or invalid records suppresses the fallback. Post-credit data prevents accepting the fallback outro as final credits. Automatic handoff still requires natural item end.

Bounded live reads for Supernatural S1E14, E15, E18, and E19 showed outro ends within 0.55–1.64 seconds of the phone's recorded playback durations. E14/E15/E18 supplied opening recaps (30/32/32 seconds); E19 supplied no recap or intro. Thus E19 can show Up Next but has no justified opening skip control.

Follow-up verification: the expanded offline marker/retry checks and existing playback reliability checks passed. `--live-supernatural` returned the real S1E15 recap 0–32 seconds and credits 2552–2599 seconds through the production client. The Debug build for the physical iPhone Air succeeded. These establish data availability and compiled behavior, not observed physical-device button taps or autoplay acceptance.

### Minimal Play next control

The follow-up design uses one 148-by-46-point flat Play next button, with a leading fill animated over the ten-second countdown. It has no metadata card or second button. Pause/buffering/lifecycle guards retain ownership of countdown ticks; Reduce Motion disables fill animation and the remaining duration remains available to VoiceOver.

### Interrupted timeline and playback recovery

An item replacement during scrubbing could leave the view's `isSeeking` flag set even though the queue and progress tracker had been reset. That prevented periodic position updates until the presentation was recreated. Item replacement now resets all three owners. Active seeks have an eight-second foreground completion deadline with request/item ownership checks; timeout reconciles the visible timeline to the player's actual clock. Foreground return re-arms pending seek deadlines, and stale callbacks cannot mutate a new seek. Completed seeks also reconcile actual position, including interrupted completions. Recovery prefers the actual clock over an optimistic UI target.

A separate one-second wall-clock watchdog detects fifteen seconds without clock movement while AVPlayer reports playing. Pause, background, seeking, and buffering do not count toward that deadline. The watchdog invokes the existing bounded recovery path. Unlike media-time callbacks, the watchdog can still run when media time stops. Deterministic checks cover seek ownership/deadlines and stall eligibility; they do not reproduce every real-provider or physical-device network stall.

The simplified button and its countdown fill were visually inspected using synthetic simulator playback (`/tmp/mello-play-next-button-fill.png`).
