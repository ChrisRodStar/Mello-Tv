# mello-jellyfin

Watch on-demand streams inside Jellyfin. Mello keeps a folder of `.strm`
files that Jellyfin treats as a normal Movies/Shows library. When you press
play, Mello finds a working source right then and relays it to Jellyfin.

```
Jellyfin app ──play──▶ Jellyfin ──opens .strm──▶ Mello /play/... ──finds a source──▶ provider sites
     ▲                                              │
     └──────────── video via Mello /stream/... ◀────┘
```

- **No transcoding in Mello.** It only passes bytes through. Jellyfin handles
  transcoding when a client needs it.
- **Library sync.** You choose titles (by TMDB ID, from TMDB lists, or from
  trending/popular collections). Mello writes the `.strm` tree, adds new
  episodes as they air, removes what you drop, and can tell Jellyfin to rescan.
- **Artwork and metadata** come from Jellyfin's own TMDB scraper, using the
  `[tmdbid-…]` tag in each folder name.

## Quick start (Docker on a Proxmox LXC or VM)

```sh
git clone <this repo> mello && cd mello
cp .env.example .env        # set MELLO_PUBLIC_URL and a TMDB key at minimum
docker compose up -d --build
docker compose exec mello cat /data/api-key   # your admin API key
```

The compose file also starts Jellyfin on port 8096. In Jellyfin, add:

| Library | Content type | Folder              |
|---------|--------------|---------------------|
| Movies  | Movies       | `/media/mello/movies` |
| Shows   | Shows        | `/media/mello/shows`  |

If you already run Jellyfin, remove its service from `compose.yaml`, point
`/library` at a host folder, and add that folder to your existing Jellyfin.

### Recommended Jellyfin settings for these libraries

Opening a `.strm` file starts a real stream lookup. Tested with Jellyfin
10.11.0: library scans and the Trickplay, Chapter Images, Keyframe Extractor
and Media Segment Scan tasks open **no** `.strm` files, with those options on,
before and after a title has been played. Only real playback opens one. They
produce nothing for these libraries either, so leave them off:

- Library → **Trickplay image extraction**: off
- Library → **Chapter image extraction**: off
- Intro/credit detection plugins: exclude these libraries (not tested; a
  plugin that runs its own ffmpeg on every file would start a lookup per file)

## Adding titles

All API calls need the admin key, sent as `Authorization: Bearer <key>`
(or `?key=<key>`).

```sh
KEY=$(docker compose exec mello cat /data/api-key)
API=http://localhost:8080/api

curl -H "Authorization: Bearer $KEY" "$API/search?q=severance"   # find TMDB IDs
curl -X POST   -H "Authorization: Bearer $KEY" $API/library/tv/95396     # add a show
curl -X POST   -H "Authorization: Bearer $KEY" $API/library/movie/27205  # add a movie
curl -X DELETE -H "Authorization: Bearer $KEY" $API/library/movie/27205  # remove
curl           -H "Authorization: Bearer $KEY" $API/library              # list
curl -X POST   -H "Authorization: Bearer $KEY" $API/library/sync         # sync now
curl           -H "Authorization: Bearer $KEY" $API/status               # health and last sync
```

Changes are synced within a few seconds. Two hands-off options:

- `MELLO_TMDB_LISTS=12345,67890`: mirror public TMDB lists, which you can
  edit from the TMDB app.
- `MELLO_AUTO_LISTS=trending_movies:40,popular_tv:20`: keep rolling
  collections. A title that drops off its list stays for
  `MELLO_AUTO_LIST_GRACE_DAYS` (default 14) before it is removed, and a title
  that gets played is kept for good (it becomes a normal library entry, so
  Jellyfin keeps its watch history; remove it through the API or admin page).

### Admin page

Open `http://<mello>:8080/admin` on a phone or computer, paste the admin key
once (it stays in that browser), then search, add, remove and sync. The page
uses the same `/api` endpoints as the examples below.

## Testing playback without Jellyfin

```sh
curl -H "Authorization: Bearer $KEY" $API/resolve/movie/27205
curl -H "Authorization: Bearer $KEY" $API/resolve/tv/1399/1/1
```

The response shows which provider and quality were picked, plus a
`stream_url` you can open in VLC.

## Configuration

Set these in `.env` (see `.env.example`).

| Variable | Default | Purpose |
|---|---|---|
| `MELLO_PUBLIC_URL` | `http://localhost:8080` | Base URL written into `.strm` files. Must be reachable by Jellyfin, and by Jellyfin apps if they direct-play. |
| `TMDB_API_KEY` / `TMDB_READ_ACCESS_TOKEN` | – | Needed for library sync and search. |
| `MELLO_API_KEY` | generated into `data/api-key` | Admin key for `/api`. |
| `MELLO_MAX_HEIGHT` | `1080` | Highest quality to look for. |
| `MELLO_AUDIO_LANGUAGES` | – | Preferred audio languages, e.g. `en`. |
| `MELLO_PROXY` | – | `socks5://` or `http://` proxy for provider and stream traffic (e.g. a gluetun VPN container). |
| `MELLO_AUTO_LISTS` | – | See above. |
| `MELLO_TMDB_LISTS` | – | See above. |
| `MELLO_SYNC_INTERVAL_HOURS` | `6` | Automatic re-sync interval. |
| `MELLO_AUTO_LIST_GRACE_DAYS` | `14` | Days a title that dropped off an auto list stays (0–365). |
| `MELLO_INCLUDE_SPECIALS` | `false` | Include Season 00. |
| `JELLYFIN_URL` / `JELLYFIN_API_KEY` | – | Trigger a Jellyfin library scan after changes. |
| `MELLO_HOST` / `MELLO_PORT` | `0.0.0.0` / `8080` | Listen address. |
| `MELLO_DATA_DIR` / `MELLO_LIBRARY_DIR` | `./data` / `./library` | State and `.strm` output. |
| `RUST_LOG` | `info` | Use `debug` to see each request. |

## Security

- **Two keys.** The `.strm` files hold a *play key*, which can only start
  playback. The admin key, used for library changes and search, is never written
  to disk outside `data/api-key`. Each playback gets its own random stream
  token.
- **The relay can only reach public internet addresses.** Playlist URIs and
  redirects pointing at your LAN, localhost or cloud metadata IPs are refused.
  Provider cookies are only sent to the site they belong to.
- **Don't port-forward Mello.** Use your LAN or Tailscale. Set
  `MELLO_BIND_ADDRESS` in `.env` to bind a single interface.
- **Logs record paths only.** Keys and tokens in query strings are never logged.

## Hardware

Mello itself needs very little: 1 vCPU, ~256–512 MB RAM, and almost no disk.
Building the image needs ~3–4 GB RAM. Bandwidth is the real limit: each stream
passes through Mello, about 5–25 Mbps depending on quality. If Jellyfin has to
transcode, give it CPU or pass an Intel iGPU through (`/dev/dri`).

## Troubleshooting

- **"no playable source"**: none of the providers had the title right now.
  Failures are remembered for 60 s, so wait a minute before retrying.
  `RUST_LOG=debug` shows per-provider detail.
- **Plays in the browser but not on a TV app**: that app is trying to
  direct-play `MELLO_PUBLIC_URL` and can't reach it. Use an address the app's
  network can see.
- **Nothing shows in Jellyfin**: check `GET /api/status` → `last_sync.failures`,
  and make sure Jellyfin can read the library folder (`PUID`/`PGID`).

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
TMDB_API_KEY=... cargo run -p stream-server
```

Crates: `server` (HTTP, library sync), `media` (provider racing, stream
probing, relay), `provider-adapters` / `provider-codecs` (per-site logic),
`core` (shared types).
