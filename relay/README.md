# Family Stream Relay

Independent Rust/Tokio service for authorized HLS and progressive MP4 delivery.
It consumes the existing Go providers' selected source and ordered backups,
creates recoverable playback sessions, and delivers media through `/stream/*`.
The Go API authorizes playback; media bytes bypass that API.

**Status:** stages 1–6 are merged. Stage 7 acceptance tooling and offline
evidence include local HLS decoding/Safari movie seek and compatibility fixes;
overall acceptance remains open.
Authorized HLS and progressive MP4 use the same
guarded, bounded streaming path and durable resource mappings, including single
ranges and If-Range. See [stage-5 evidence and measured limits](docs/stage-5.md).
Explicit ordered failover, prepared source refresh, runtime health/metrics and
bounded drain are implemented; see [stage-6 evidence](docs/stage-6.md). Runtime
readiness reflects component state. Overall live/player/device/deployment
acceptance remains false; packaging is deferred.

See [stage-7 results and blockers](docs/stage-7.md) and the
[local live execution plan](docs/stage-7-live-plan.md).
Track progress in the [stage TODO](TODO.md).

## Read before implementation

| Document | Purpose |
| --- | --- |
| [AGENTS.md](AGENTS.md) | Project instructions and engineering requirements |
| [Stage TODO](TODO.md) | All seven delivery stages, current progress and completion gates |
| [Architecture](docs/architecture.md) | Processes, ownership and Rust module boundaries |
| [Provider handoff](docs/provider-handoff.md) | Exact mapping from current `providers.v1` Go models |
| [HTTP/session contract](docs/contracts.md) | Private control API and authorized stream interface |
| [Authentication and recovery](docs/sessions.md) | Grants, durable state, expiry, renewal and revocation |
| [Controlled networking](docs/networking.md) | Mullvad, SSRF, DNS, TLS, redirects and credential scopes |
| [HLS delivery](docs/hls.md) | Master/rendition retention and all supported URI/resource kinds |
| [Media and ranges](docs/media.md) | Incremental streaming, MIME, GET/HEAD and byte ranges |
| [Failure and failover](docs/failover.md) | Backup activation without cross-provider segment splicing |
| [Configuration](docs/configuration.md) | Explicit limits, units and startup invariants |
| [Operations](docs/operations.md) | Supervision, health, draining, persistence and three containers |
| [Verification](docs/verification.md) | Offline and authorized live acceptance matrix |
| [Implementation plan](docs/implementation-plan.md) | Ordered tasks with concrete completion gates |
| [Evidence and sources](docs/evidence.md) | Audited local code, limitations and primary references |

Machine-readable contract artifacts (runtime coverage is staged):

- [OpenAPI 3.1](contracts/openapi.json)
- [Relay session schema](contracts/relay.schema.json)
- [Current Go provider selection schema](contracts/providers.schema.json)
- [Synthetic session import](contracts/examples/create-session.json)
- [Synthetic configuration](config/relay.example.toml)

The new `relay.v1` control API is specified here, not already agreed/implemented
by the API project. Provider wire compatibility is grounded in actual Go models;
the paired providers HTTP boundary now wraps the native engine.
[HTTP integration and evidence](docs/http-integration-acceptance.md) covers its
running synthetic service-to-relay regression and outstanding live checks. That importer
must use the same production session path, not a second preview relay.

## Service boundary

```
Client -> Tailscale Serve -> /api/* -> Go API
                         -> /stream/* -> Rust relay -> Mullvad -> upstream media
Go API -> private providers.v1 lookup -> Go providers -> Mullvad -> provider sites
Go API -> private relay.v1 control -> Rust relay -> relay-owned encrypted session DB
Catalog -> own SQLite / TMDB outside Mullvad
```

Exactly three eventual application-stack containers: application (API + built
web), catalog, and streaming (providers + relay + Mullvad/firewall). Tailscale
Serve stays on the host. Development is local on this Mac; no production deployment
is required. No separate VPN container or separate public backend address.

## Implementation target

Deliver authenticated HLS VOD and progressive MP4 with accurate byte ranges,
full rendition associations, incremental backpressure, cancellation, controlled
networking, recoverable state and explicit failover. Provide health/control
interfaces and deployment-compatible lifecycle behavior. Support all four current
providers through their common contract, not provider-specific relay branches.

DASH playback, transcoding, downloads/offline libraries, CDN cache distribution,
LL-HLS and seamless arbitrary provider switching are outside `relay.v1`.
Unsupported formats/extensions must fail explicitly, never produce misleading
playlists. See the feature matrix in the protocol documents.

Implementation starts from [the complete plan](docs/implementation-plan.md).
Builds, tests, benchmarks and deployment require explicit authorization.

Receive-boundary shutoff correction: see [opt-in acceptance owner and accounting](docs/acceptance-run.md). The corrected tooling is verified offline; a fresh live run still requires approval.

Fresh coordinated live approval is pending: [exact proposed plan](docs/http-integration-live-plan.md).
