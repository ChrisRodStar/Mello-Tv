# Status and next steps

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

## Next
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
