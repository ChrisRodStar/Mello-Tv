# Verification layout

No executable Rust tests exist yet. Implement the acceptance matrix in
[verification](../docs/verification.md) alongside the production modules. Default
tests must be offline, deterministic, bounded and independent of Mullvad. Synthetic
fixtures must never contain real source URLs, tokens, media bodies or credentials.

- `contract/`: actual Go envelope compatibility and relay request/response schemas.
- `integration/`: production handlers/store/transport with injected offline peers.
- `fixtures/hls/`: playlists covering rendition closure, keys, maps and byte ranges.
- `fixtures/http/`: range and response behavior cases.
- `fixtures/security/`: destination, credential and resource ownership cases.
