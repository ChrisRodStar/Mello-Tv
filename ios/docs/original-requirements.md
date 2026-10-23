# Family Stream iOS

Native SwiftUI client using AVKit for playback. Consume the same stable public Go API contract as the React/Vite client through one private Tailscale address. Use functional `/api/*` routes and authorized `/stream/*` playback URLs. Keep private backend addresses and upstream provider URLs out of the client.

Support profiles, browsing, search, personal libraries, targeted resume lookups, watch progress, and playback through the established contract. Preserve local UI state when saving progress rather than fetching the complete library on each update. Show available cached catalog data and actionable playback errors during service outages.

Maintain this as its own project and release artifact; it is installed on Apple devices, not run in a Pi container. Implement only after identifiers and catalog/provider/relay response shapes are established. This scaffold does not establish physical-device playback or seeking behavior.

Tests, builds, benchmarks, device installation, and deployments require an explicit user request.
