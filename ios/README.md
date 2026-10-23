# Family Stream iOS

This SwiftUI/AVKit app connects to the Go public `api.v1` contract.
Configure your server origin in `ServerAddress.swift` (e.g. your private server or local development host `http://127.0.0.1:8090`).

## Private household access

The app opens directly into family profiles. There is no username/password,
account gate, Keychain session requirement or sign-out control. The API uses the
existing household storage owner; profile IDs, favorites, library and progress
remain unchanged.

API requests carry no account bearer or cookies; relay playback retains its
separate signed capabilities and private upstream guards.

The adapter decodes the record envelopes and maps them into the UI
models. Profiles use server-allocated IDs; favorites are a separate API collection.
Home/Movies/TV combine retained browse shelves with profile recommendations.
Search combines saved catalog matches with the available first-page discovery
snapshot. Details/episodes wait within a bounded window for queued metadata
refreshes. Genre/favorites choices use saved onboarding shelves. Artwork loads
directly from the allowlisted TMDB image CDN without credentials.

Library/history and progress come from the same server-owned household data as the
web app. Progress uses real playback IDs, increasing sequence numbers and measured
cumulative play time; writes are serialized/coalesced. Active profile selection
remains local per server.

## Playback

The player requests landscape-only orientation in either direction for its own
window, retaining the policy across episode changes and menus. Closing playback
releases that policy and requests the previous app orientation. Speed uses a
compact horizontal chooser over the video; audio/subtitles use a small two-column
overlay with scrollable track choices. Both dismiss with Close or a tap outside.
Only episode browsing uses the larger sheet presentation.

The custom player uses an unframed title/close row and central play/pause control,
an accessible red-accent timeline with remaining time, and a flat bottom action
row. Playback speed supports 0.5–2× and survives episode changes; TV playback has
an episode/season picker and an explicit Next Episode action with season-boundary
handling. Metadata loading failures are retryable and do not stop the current
video. Lock hides the controls and disables seek/brightness/volume gestures;
tap the video to reveal Unlock. Double-tap the video on the left/right to skip
back/forward ten seconds. Vertical swipes on
the left adjust screen brightness, and swipes on the right adjust system media
volume, with a temporary level indicator. Brightness restores on dismissal or
backgrounding; volume remains at the viewer's chosen level. Gestures leave the
central control and top/bottom bars alone; VoiceOver retains named seek actions.
Source lookup and initial video buffering have
separate messages; close remains available throughout. Controls stay visible
while paused, buffering, scrubbing or using VoiceOver. The audio/subtitle sheet
lists media selection options reported by the loaded AVPlayer item.

Play creates a profile-scoped API playback session, advertises supported HLS/MP4
and AVC/AAC capabilities, and validates identity plus the same-origin `/stream/`
URL. AVPlayer receives only the signed relay capability, never the API bearer or
an upstream source URL. The server's resume position and playback ID are retained.
Continue Watching and title details may prepare one likely playback target on
an unconstrained connection. A preparation owns its authorization and optional
paused AVPlayer together, transfers once, and cannot publish after cancellation.

Failed items and prolonged buffering trigger bounded automatic recovery through
the backup and fresh-source routes. Renewal and recovery share mutation
ownership, preserving operation keys, position and cumulative progress identity.
The player exposes manual retry after its automatic recovery budget is exhausted.

## Building the iOS App

Open `FamilyStream.xcodeproj` in Xcode:

1. Select your target device (Simulator or connected iOS device).
2. Under **Signing & Capabilities**, select your personal Team.
3. Build and run.

To build via the command line:

```bash
xcodebuild -project FamilyStream.xcodeproj -scheme FamilyStream \
  -configuration Debug -destination 'generic/platform=iOS' \
  -derivedDataPath /tmp/family-stream-build \
  -allowProvisioningUpdates build
```
