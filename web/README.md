# Family Stream Web

Private family app with no account sign-in, password or sign-out screen. Opens directly into household profiles; Tailscale is the access boundary. Internal service tokens and signed stream capabilities remain server-managed.

React/Vite client consuming the same stable public Go API contract as the SwiftUI client. Use one private Tailscale address, functional `/api/*` routes, and authorized `/stream/*` playback URLs. Keep private backend addresses and upstream provider URLs out of the client.

Support profiles, browsing, search, personal libraries, targeted resume lookups, watch progress, and playback through the established contract. Update local progress state without retrieving the full library on every save. Use retained catalog data during upstream outages and present actionable playback failures.

Maintain the web client as its own project and release artifact. Package its built static assets with the Go API in the application container; do not add a separate web container. Define atomic asset release activation so web updates can happen without restarting the API process. The Go API serves the assets; Vite is the client build/development tool.

Implement against established catalog/playback contracts rather than guessed response shapes. The reference React/Vite client has been copied into this independent project and connected to api.v1. Tests, builds, benchmarks, and deployments require an explicit user request.

Favorites onboarding reads qualified `favorites-*` cached shelves through `/api/v1/browse`, balances the selected genres and movie/TV format, and caps choices at 48 unique titles. Cached `onboarding-*` movie shelves are a same-genre cold-cache fallback. Both shelf families are hidden from general home sections. Posters load directly from the allowlisted TMDB image CDN with browser HTTP caching and a no-referrer policy. Network failures get bounded retries. Image bytes are not stored by the catalog or API.


## Local development

Install with `bun install --frozen-lockfile --ignore-scripts`; use `bun run dev` for the loopback frontend at http://127.0.0.1:5173. Development proxies `/api` to the Go API at port 8090 and `/stream` directly to the relay at port 8095. Optional server-only overrides are `FAMILY_STREAM_API_ORIGIN` and `FAMILY_STREAM_STREAM_ORIGIN`. Provision services and secrets separately; do not put secrets in Vite variables.

Authorized checks: `bun run typecheck`, `bun test`, and `bun run build`. `tools/browser-fixture.ts` is an opt-in synthetic harness, not the real service or evidence of real provider playback. For production, package `dist/` with the Go API and route streams directly to the relay. Linux VPN guards and deployment packaging still need implementation before the Raspberry Pi cutover.
