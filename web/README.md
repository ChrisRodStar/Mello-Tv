# Mello TV web client

React/Vite viewing client for the Rust server in `crates/server`. Production
serves the built website, `/api/*`, and `/stream/*` from one origin. Profiles
share one household installation; network access is the access boundary.

## Development

Start the backend using the [Docker setup](../docs/DOCKER.md), then:

```sh
bun install --frozen-lockfile --ignore-scripts
bun run dev
```

Open <http://127.0.0.1:5173>. Vite proxies the API and streams to
`http://127.0.0.1:8080`. Set the server-only `MELLO_SERVER_ORIGIN` environment
variable to use a different backend. Never put credentials in Vite variables.

## Checks

```sh
bun run typecheck
bun test
bun run build
```

The production build is packaged with the server in the Mac app and Docker
image. Unit and contract tests do not establish real provider playback.

## Data and playback

Use the existing public API contracts for profiles, browsing, search, saved
lists, progress, and playback. Keep upstream provider URLs and credentials out
of the client. The player accepts authorized same-origin stream URLs.

Onboarding uses cached `favorites-*` shelves, with same-genre `onboarding-*`
movie shelves as a fallback, and caps title choices at 48. Both shelf families
stay out of home sections. Posters load directly from the allowlisted TMDB CDN;
image bytes are not stored by the server.
