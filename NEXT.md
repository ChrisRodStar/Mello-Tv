# Status and next steps

## Round 3 (2026-10-30, tested on an i3-7100T / 4 GB Ubuntu 22.04 box)
- **Docker image builds** on real hardware: full `docker build` in ~3 min,
  image 158 MB (was NEXT #3).
- **Trickplay/chapter images (was NEXT #1): no background opens.** Jellyfin
  10.11.0 against the fake server, both libraries with trickplay + chapter
  extraction on (also "during library scan"): scans and the Trickplay, Chapter
  Images, Keyframe Extractor and Media Segment Scan tasks opened **zero**
  `.strm` files, both before any play and after each title had been played
  (when Jellyfin knows runtime and codecs). The only opens were the 3 real
  plays (one each, `Lavf/61.7.100`). README updated; the "leave them off"
  advice stays because they produce nothing for these libraries.
- **Auto-list grace period + keep on play (was NEXT #5):**
  `MELLO_AUTO_LIST_GRACE_DAYS` (default 14). `sync-state.json` records when an
  auto list last included each title; one that drops off stays until the grace
  ends (and indefinitely while a list fails to load). Playing a title that is
  only there because of an auto list turns it into a manual entry
  (`Library::note_played`, called from `/play`), so it never disappears with
  its watch history. TMDB lists (`MELLO_TMDB_LISTS`) are unaffected: removing
  a title from your own list still removes it. Test:
  `auto_list_titles_get_a_grace_period_and_played_ones_stay`.
- **Admin page (was NEXT #6):** `GET /admin` (static HTML/JS/CSS embedded with
  `include_str!`, strict CSP, `X-Frame-Options: DENY`). It asks for the admin
  key once (localStorage) and uses the existing `/api` endpoints: search, add,
  remove, sync, status. Server data is only ever inserted with `textContent`.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings` and
  `cargo test --workspace` all pass (server: 13 unit + 5 end-to-end).

## Next (round 4)
1. **Mid-play source expiry (old #4) — design, not built.** Swapping a fresh
   source behind the same token is only safe when the new source has the same
   layout, because the client keeps using what it already fetched:
   - HLS: the relay names resources by a hash of their upstream URL
     (`MediaRelay::resource_id`). Signed URLs change on refresh, so the
     client's cached playlist would 404 (`ResourceNotFound`). Plan: on an
     upstream 401/403/404/410 for a session whose `expires_at` is near or past,
     re-resolve the **same provider and variant** (`refresh: true`), fetch the
     new media playlist, and accept it only if segment count and every
     `#EXTINF` duration match; then remap each old resource id to the new URL
     at the same position and retry the failed request once. Otherwise return
     the error (Jellyfin re-opens `/play`, which re-resolves because the cached
     source is expired).
   - MP4: accept the new URL only if `Content-Length` (and ideally an
     `ETag`/first-range checksum) matches, then keep serving byte ranges.
   - Needs: per-session ordered segment list per media playlist, a "refresh"
     hook from the relay back to the engine, a test where the fake CDN starts
     returning 403 for old URLs mid-stream.
2. **First real-hardware test with real sources** (old #2) once the providers
   are replaced.
3. Optional: Jellyseerr integration; HEVC/4K preference knob.

## Round 2

## Verified (round 2)
- End-to-end tests (`crates/server/tests/end_to_end.rs`): fake provider plus two
  fake CDNs, real ffmpeg-generated HLS and MP4, played through `/play` by the
  real `ffmpeg`/`ffprobe`. These also pass with **Jellyfin's ffmpeg 7.1.2**
  (`MELLO_TEST_FFMPEG` / `MELLO_TEST_FFPROBE` point at wrapper scripts).
- Real **Jellyfin 10.11** against the fake server:
  - The library scan opens **no** `.strm` URLs. TMDB IDs come from folder names.
  - The first play probes once (handles a 22 s provider search with no
    timeout). Playback then reuses the session.
  - HLS and MP4, movies and episodes, play as static streams and as HLS remux.
  - Background tasks open nothing with the default library options.
- Jellyfin's ffmpeg rejects disguised `.png` segments when hit directly, but
  plays them through Mello. The extension rewriting is required.

## Fixed in round 2
- Playlists now use absolute URLs built from the request's Host, so streams
  Jellyfin proxies no longer return 404.
- Sessions are reused while actively streaming, so a seek doesn't trigger a new
  provider search.
- The next episode is resolved in the background, so autoplay starts instantly.
- A failed TMDB list fetch never deletes titles (which would wipe watch history).

## Next (as of round 2; see round 3 above)
1. **Trickplay/chapter images**: test whether turning them on makes Jellyfin
   open every `.strm` (the test was interrupted). If so, keep the README
   warning, or detect background probes.
2. **First real hardware test**: run `docker compose up`, add a title, play it.
   Watch the logs for which providers win, how long resolves take, and how
   often they fail.
3. **Docker image build**: confirm the full `docker build` works (the sandbox
   could not reach Debian mirrors).
4. **Mid-play source expiry**: if a provider URL expires during a long pause,
   segments fail. Consider re-resolving and swapping the session's source
   behind the same token.
5. **Auto lists remove titles that drop off trending**, which loses watch
   history. Consider keeping titles that have been watched, or a grace period.
6. Optional: a tiny admin page for adding titles from a phone (instead of
   curl). Optional: Jellyseerr integration.
7. Optional: HEVC/4K preference knob (`MELLO_MAX_HEIGHT=2160` exists. Codecs
   are fixed to H.264/AAC preference).

## Manual Jellyfin harness
```sh
MELLO_FAKE_LIBRARY=/tmp/lib MELLO_FAKE_SECS=900 MELLO_FAKE_DELAY_SECS=0 \
  cargo test -p stream-server --test end_to_end manual -- --ignored --nocapture
docker run --network host -v /tmp/lib:/media jellyfin/jellyfin:10.11.0
```
