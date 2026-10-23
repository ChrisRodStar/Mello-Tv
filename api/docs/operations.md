# Local operation and recovery

Build with Go 1.27 or newer: `go build -o .local/api ./cmd/api`. Run
`.local/api /private/api.json`. Configuration and secrets must be regular private
files (no final symlinks; no group/other permissions). Keep their directory mode
0700 outside Git. No `.env` discovery, Keychain access, system VPN changes,
remote-host access or paid identity service is involved.

Configuration is strict JSON with these required fields:

| Field | Purpose / local example |
|---|---|
| `listen` | Literal loopback listener; default `127.0.0.1:8090` |
| `db_path` | API-owned persistent SQLite path |
| `origin` | Exact public client origin; `https://household.example` or local `http://127.0.0.1:8090` |
| `catalog_url` | Private literal loopback URL, e.g. `http://127.0.0.1:8092` |
| `provider_url` | Private literal loopback URL, e.g. `http://127.0.0.1:8093` |
| `relay_url` | Private control URL, e.g. `http://127.0.0.1:8094` |
| `catalog_token_file`, `provider_token_file`, `relay_token_file` | Separate 32–4096-byte bearer files without whitespace/newline |
| `household_account_id` | Optional explicit legacy storage owner; normally reuse the sole existing owner automatically |
| `setup_token_file` | Deprecated and ignored; retained only for config compatibility |
| `signing_key_file` | Ed25519 PKCS#8 DER or PEM, separately provisioned to the relay as a public key |
| `encryption_key_file` | Exactly 32 raw random bytes, retained independently of database backups |
| `kid` | Relay-allowlisted signing key identifier |
| `web_dir` | Optional independently built web asset directory; no client is built here |

The three private services and API must use distinct ports. Configure the relay's
stream listener separately (e.g. 8095): both catalog and relay's old stream default
are 8092. The API does not need the stream host: all output is relative `/stream/*`.
The single future Tailscale Serve entry sends `/api/*` to the API and `/stream/*`
directly to that stream listener. Private control stays unexposed. This local
implementation requires loopback private endpoints; container/private-channel
packaging remains future deployment work under the three-container architecture.

Household clients do not sign in. Startup reuses a pinned owner or the sole legacy
account, retaining all profile IDs and personal data. Fresh storage automatically
creates an internal owner with no usable login credentials. Multiple legacy owners
require explicit `household_account_id`; an unknown configured owner fails startup.
The configured public origin and cross-site mutation protection remain enforced.
Keep the API reachable only over the private family network. Internal service
bearers and relay signing/state-encryption keys are still required.

The deprecated `setup_token_file` field is accepted but never opened. Existing
bootstrap/key files and legacy password/session rows can be retained privately for
rollback; no client can log in, create accounts or change passwords through them.

SQLite schema v1 migrates transactionally, refuses newer schemas, enables WAL,
FULL synchronous commits, foreign keys and a three-second busy bound. Files are
0600, newly created directories 0700. A retained advisory file lock enforces one API process per database; process exit releases ownership. Personal
state and retained metadata survive process restart. Artwork bytes are excluded from snapshots; browsers fetch and cache images from TMDB’s image CDN. A separate AES-256-GCM key
protects pending provider selections/control payloads, with per-row associated
data and random nonces. A persistent authenticated sentinel rejects a wrong key
at startup; never replace a lost key or reset storage to claim recovery. Signed
playback URLs are generated only for replies and are never stored in SQLite.

Four independent owned workers handle snapshots, recommendation synchronization, metadata refresh delivery, and revocations/maintenance. Periodic passes run each minute; requested metadata wakes the refresh and snapshot owners promptly, coalesced at a 500 ms minimum interval. Each pass has a 40-second
bound and joins on shutdown. Revocations do not wait behind catalog work.
Recommendation deletion outbox runs before replacement inputs; source/profile
changes during synchronization leave durable work for retry. Inputs synchronize
in bounded batches with fair attempt ordering. Catalog owns recommendation build
cadence (15-minute staleness scan, active 24 hours / inactive seven days).

HTTP private clients disable environment proxies, redirects and decompression,
limit response headers to 16 KiB and connections to four/host, and cap response
bodies (192 MiB full snapshot, 2 MiB selection, 32 KiB relay reply). Snapshot calls
have 30 seconds; catalog updates three seconds; provider resolution 35 seconds;
relay control 12 seconds, all clipped to request/shutdown contexts. Public HTTP
has 64 accepted connections, 32 admitted handlers, 8 KiB headers, 3-second header,
5-second read, 45-second write and 30-second idle limits. Playback control uses
immediate bounded admission; Household,
profile, list, snapshot, refresh-queue and playback-record bounds reject new work
rather than evicting useful personal state. Current playback-record cap is 4096
including the seven-day retry retention window. Each playback keeps at most 256
successful mutation keys. These are resource policies, not viewer capacity claims.

Snapshot receipt validates the complete envelope, identities, exact coordinates,
record keys/revisions/times, duplicate records, recognized metadata kinds and validated artwork references. Legacy image payload records are discarded before persistence. It rejects truncated responses,
malformed JSON, unknown versions and revision/time regressions, then atomically
replaces SQLite state before publishing the new in-memory copy. Failed receipt,
validation or commit preserves the prior copy. Freshness changes over time even
without writes. A catalog restored to an older revision is not silently allowed
to roll API state backward; reconcile its durable revision before resuming updates.

`/health/livez` and `/health/readyz` report only API lifecycle/storage status.
Readiness does not require all private services to be up, so cached browsing can
remain available. Authenticated `/api/v1/status` reports aggregate snapshot/outbox
state without source/session/account identifiers, secrets or arbitrary errors.
No access logs, URL logs, telemetry or raw upstream error logs are enabled.

SIGINT/SIGTERM stops admission, cancels request/worker contexts, drains HTTP for
five seconds, closes remaining sockets, joins admitted handlers and all workers,
closes idle transports and finally SQLite. Existing relay resources continue
independently until expiry/revocation/source failure. Open API TCP connections
are not preserved across process replacement.

For a backup, stop and join the API first, then copy the closed database to a
private destination using the operator's backup procedure; retain the matching
state encryption key separately. Do not copy a changing SQLite main file alone.
Restore a matching artifact/database/key set. Reconcile post-backup revocations
or invalidate affected relay sessions before traffic resumes; restoration must
not resurrect grants. Future migrations require backup/rollback tests before
raising schema version. Unknown/newer schemas and corrupt retained snapshots
fail startup without deleting/resetting data. There is no automatic corruption
repair, password recovery backdoor, destructive eviction or distributed controller.

On upgrade, the API removes image payloads from its retained snapshot and vacuums/checkpoints SQLite to reclaim their storage. Authentication, profiles, libraries and playback state are preserved. Legacy image routes issue redirects; no API image download or image-byte persistence is performed.

For isolated same-host container deployment, `trusted_private_ips` explicitly
allowlists literal RFC1918 listener/service addresses in addition to loopback.
Hostnames, wildcard/public binds and redirects remain rejected. Keep these
addresses on the operations-owned Docker bridge, require existing per-service
bearers, and publish only API/web on host loopback. Control traffic leaving this
isolated host network requires an authenticated encrypted channel.
