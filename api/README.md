# Family Stream API

Runnable Go public API for the independent Family Stream services. Owns household
profiles/onboarding/preferences, watchlists/favorites, measured
viewing/progress, durable catalog/artwork snapshots, recommendation-input
synchronization and provider-to-relay playback coordination.

The public client contract is `/api/v1`; media uses authorized same-origin
`/stream/*` URLs served directly by the Rust relay. Known-ID playback never waits
for the catalog. Cached browsing/artwork and household state survive API restart.
Private Selection payloads retain provider order and are encrypted during pending
orchestration; no upstream media URL or raw metadata reaches public clients.

- [Public routes and retry semantics](docs/api.md)
- [OpenAPI](contracts/openapi.json)
- [Configuration, limits, backup and recovery](docs/operations.md)
- [Verification and evidence boundaries](docs/verification.md)

```sh
go build -o .local/api ./cmd/api
.local/api /private/api.json
```

Provision private configuration/key files as documented; no credentials are
bundled. Default checks are offline and credential-free:

```sh
gofmt -l cmd internal
go vet ./...
go test -race -count=1 ./...
go build -o .local/api ./cmd/api
```

The explicit `python3 tools/production_listeners.py` integration check uses sibling
provider/relay source and Go/Rust tools. It runs production HTTP/control/stream
listeners with synthetic adapter/probe/network boundaries in a disposable relay
source copy; it neither edits siblings nor uses live providers/Mullvad. See the
verification document for exact evidence and remaining live-playback gaps.

Household access relies on the private network. There is no app login, password,
setup key, session cookie or account-management endpoint. Existing profile owners
and all personal state are retained. See operations for legacy owner selection.

Development remains local on the Mac. Six repositories and the three-container
architecture remain independent. Clients, deployment, Server/device operation,
container supervision and new live playback traffic are outside this delivery.
