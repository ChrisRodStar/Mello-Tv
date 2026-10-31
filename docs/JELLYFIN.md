# Jellyfin

Mello TV can export a profile's **My List** as a Jellyfin movie/show library. Jellyfin provides its own browsing, users, watch history, and transcoding; Mello resolves sources when playback starts and delivers them through its existing relay. This is a Mello integration with Jellyfin's native `.strm` support. It does not require a Jellyfin plugin or imply endorsement by the Jellyfin project.

This integration is opt-in and currently configured through server environment variables. There is no Mac settings control for it. Existing Mello clients continue to work.

## Set up

1. Create a Mello profile, open it in the web app, and add movies or shows to **My List**. A separate profile makes it easy to curate the Jellyfin library. Obtain its `id` from `GET http://SERVER-IP:8080/api/v1/profiles`.
2. Choose a new, dedicated export directory writable by Mello and readable by Jellyfin. Do not use an existing media collection or a path containing symlinks. Non-regular files such as FIFOs and devices are also refused.
3. Generate a playback key with `openssl rand -hex 32`. Store it with your server configuration; keep the same value across restarts.
4. Set all four variables below on the Mello server, then restart it. Partial configuration is rejected at startup.

| Variable | Value |
| --- | --- |
| `STREAM_JELLYFIN_PROFILE_ID` | The Mello profile ID whose My List should be exported. |
| `STREAM_JELLYFIN_LIBRARY_DIR` | Export directory as seen by the Mello process, e.g. `/jellyfin-library`. |
| `STREAM_JELLYFIN_PUBLIC_URL` | Mello's HTTP(S) origin reachable by Jellyfin and its players, e.g. `http://192.168.1.20:8080`. No URL path, query, fragment, username, or password. |
| `STREAM_JELLYFIN_PLAY_KEY` | A random hexadecimal string of 32–128 characters; the command above produces 64. |

For the repository's Docker Compose setup, an additional `compose.override.yaml` can supply the configuration and writable bind mount:

```yaml
services:
  mello:
    environment:
      STREAM_JELLYFIN_PROFILE_ID: "${STREAM_JELLYFIN_PROFILE_ID}"
      STREAM_JELLYFIN_LIBRARY_DIR: /jellyfin-library
      STREAM_JELLYFIN_PUBLIC_URL: "${STREAM_JELLYFIN_PUBLIC_URL}"
      STREAM_JELLYFIN_PLAY_KEY: "${STREAM_JELLYFIN_PLAY_KEY}"
    volumes:
      - ./jellyfin-library:/jellyfin-library
```

Set the three substituted values in a local `.env` file, create the directory and grant the image's UID 10001 write access (`mkdir -p jellyfin-library && sudo chown 10001 jellyfin-library`), then run `docker compose up -d --build --wait`. Jellyfin also needs directory traversal and read access. Keep that `.env` file private. Add `.env`, `compose.override.yaml`, and `jellyfin-library/` to this checkout's `.git/info/exclude` so local credentials and exports stay untracked. Your existing `mello-data` volume remains necessary.

5. Give Jellyfin read access to the same host directory. For a Jellyfin container, mount `./jellyfin-library` at a path such as `/media/mello:ro`.
6. In Jellyfin's Dashboard → Libraries, create **Movies** with `/media/mello/movies` and **Shows** with `/media/mello/shows`. For a native Jellyfin installation, use the equivalent host paths. Enable TMDB metadata lookup; Mello writes `[tmdbid-ID]` tags for reliable matching.
7. Run a Jellyfin library scan. Mello synchronizes on startup and every 60 seconds. Local filesystem real-time monitoring can detect later changes; network mounts may require scheduled or manual Jellyfin scans. Mello does not currently call Jellyfin's refresh API, so no Jellyfin API key is required.

`localhost` inside a Jellyfin container points to that container. Use a Mello address reachable from the Jellyfin host and any client that opens remote streams directly. The export folder and Mello's network address are separate requirements.

## What is synchronized

Movies become `.strm` files under `movies/`. Shows become season folders and one `.strm` file for each episode with a known air date on or before the current UTC date. Unknown or future air dates are omitted. Source availability is checked at playback time, so an indexed title does not guarantee a currently playable provider.

Adding or removing a My List title updates the export on the next successful synchronization. Jellyfin sees removals on its next scan; removing titles can affect Jellyfin's library/history behavior. Mello does not change the profile's list or watch history. Jellyfin viewing progress is stored in Jellyfin and is not synchronized back to Mello.

Mello maintains `.mello-jellyfin.json` to track files it owns. It preserves unrelated files and refuses to overwrite or remove a managed file whose contents were edited externally. Catalog lookup failures stop synchronization before file changes, so a temporary metadata failure does not erase the library. If a season response contains fewer episodes than the show summary promises, Mello retains the previous export and retries rather than publishing an incomplete replacement. Once generated, owned folder and file names stay stable when metadata titles change, preserving Jellyfin item paths. Keep the ownership manifest with the export. Do not manually edit generated `.strm` files; change the configuration or My List instead. Exports are limited to 20,000 files; ownership manifests are limited to 32 MiB and individual `.strm` files to 8 KiB. Exceeding a limit stops synchronization and reports an error.

## Playback and credentials

Each `.strm` contains a stable Mello URL, not a provider URL or an expiring relay URL. A request checks the playback key and confirms the title/episode still belongs to the configured library, resolves a source, then redirects to an in-memory relay session. HLS playlists use absolute relay URLs so they also work when Jellyfin proxies the playlist through its own HTTP endpoint. Bridge resource URLs include media suffixes so FFmpeg's HLS segment-extension checks accept them. Mello handles upstream request headers and delivery; Jellyfin handles any necessary transcoding.

The export key grants playback of the selected library. Treat the exported directory and its backups as credentials. It does not grant library editing or profile management. To rotate it, change `STREAM_JELLYFIN_PLAY_KEY`, restart Mello, allow the export to synchronize, and rescan Jellyfin. Restarting invalidates the previous in-memory relay sessions, and old bridge URLs stop resolving. Proxy access logs should omit query strings because the stable URL includes the key.

Initial playback includes provider lookup and Jellyfin's remote media probe. Failures are surfaced as HTTP errors, not a fabricated video. The bridge reuses a resolved source for five minutes or while relay delivery remains active, capped by a known source expiry. It cannot guarantee recovery when a provider expires or disappears during viewing. Stop and restart playback to request a new source. There is no automatic cross-provider mid-stream failover. Removing a title rejects new requests through its stable URL; an already issued relay capability can continue until server restart or session eviction.

## Verification and acceptance

The interoperability check used an isolated **Jellyfin 12.4** server with **jellyfin-ffmpeg 7.1.4-3**, synthetic H.264/AAC media, and a local HTTP fixture. A second pass used the production Mello exporter, playback route, and relay, with only provider selection replaced by a deterministic fixture:

- A Movies scan recognized the TMDB identifier in the folder name.
- A Shows scan recognized the series TMDB identifier and season 1, episode 1.
- The scan made no requests to the `.strm` playback URLs.
- Jellyfin's `PlaybackInfo` probe followed an HTTP 307 redirect through a master playlist, media playlist, and segment, reporting the expected eight-second H.264/AAC source.
- Jellyfin's static remote-stream endpoint returned the master playlist unchanged, preserving its absolute nested playlist URL.
- Actual Mello-generated movie and episode exports were indexed and probed through the production bridge and relay. FFmpeg successfully sought to three seconds and decoded two seconds of both sources through Jellyfin's static HTTP proxy, without relaxing extension checks.

These checks establish Jellyfin's file/probe/proxy behavior with controlled media. They do not establish real provider playback, successful transcoding, real-device seeking/pause/resume, or compatibility with every Jellyfin client. Normal media scans skip shortcut probing in the inspected Jellyfin source; third-party analysis plugins may behave differently. Leave chapter/trickplay extraction off for this remote library unless independently tested.

For a deployment acceptance check:

1. Add one movie and one show with an aired episode to the configured profile. Wait for the `Jellyfin library export synchronized` log, then scan both Jellyfin libraries. Confirm the correct titles and episode numbers.
2. Play each in Jellyfin Web and the intended TV/mobile client. Confirm the first frame and audio, seek forward/backward, and pause/resume. Check transcoding separately if that client requires it.
3. Restart Mello and confirm the same `.strm` URLs still start playback. Test during an idle period; active relay sessions do not survive a server restart.
4. Remove a test title from My List. After synchronization and scanning, confirm its generated file and Jellyfin entry disappear while unrelated local files remain.
5. Check missing/wrong playback keys are refused, then rotate the key and confirm a rescan restores playback with the new export.

If nothing appears, check the selected profile's My List, Mello's `Jellyfin library export failed` logs, the container mount paths and permissions, and Jellyfin's scan status. If an item appears but will not play, verify network reachability of `STREAM_JELLYFIN_PUBLIC_URL` from both the Jellyfin host and client before checking provider availability.

## Integration basis

The integration was designed against Jellyfin's documentation and server source, independently of third-party fork implementations:

- [Movies organization and naming](https://jellyfin.org/docs/general/server/media/movies/), [TV shows](https://jellyfin.org/docs/general/server/media/shows/), and [metadata provider identifiers](https://jellyfin.org/docs/general/server/metadata/identifiers/).
- [Filesystem storage requirements](https://jellyfin.org/docs/general/administration/storage/) and [scheduled tasks](https://jellyfin.org/docs/general/server/tasks/).
- Native `.strm` recognition in [BaseVideoResolver](https://github.com/jellyfin/jellyfin/blob/v12.4/Emby.Server.Implementations/Library/Resolvers/BaseVideoResolver.cs#L147), scan-time probe gating in [FFProbeVideoInfo](https://github.com/jellyfin/jellyfin/blob/v12.4/MediaBrowser.Providers/MediaInfo/FFProbeVideoInfo.cs#L87), and playback-time probing in [MediaSourceManager](https://github.com/jellyfin/jellyfin/blob/v12.4/Emby.Server.Implementations/Library/MediaSourceManager.cs#L182).
- Static HTTP proxy behavior in [FileStreamResponseHelpers](https://github.com/jellyfin/jellyfin/blob/v12.4/Jellyfin.Api/Helpers/FileStreamResponseHelpers.cs#L29).

A Jellyfin plugin or Live TV tuner interface would add a separate deployment/API contract and would not fit a normal on-demand Movies/Shows library as directly. Native shortcuts let Mello keep its existing resolver, relay, profiles, and clients while Jellyfin owns its library and playback UI.
