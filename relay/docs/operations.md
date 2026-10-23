# Process lifecycle and eventual deployment

## Established topology

Exactly three application-stack containers:

1. Application: Go API plus separately built web assets; outside Mullvad.
2. Catalog: Go plus catalog-owned persistent SQLite/artwork; outside Mullvad.
3. Streaming: Go providers, Rust relay and Mullvad tunnel/firewall; separate
   supervised processes within that one container.

Tailscale Serve runs on the Pi host and exposes one private address. Route `/` to
application web, `/api/*` to Go API, and `/stream/*` directly to the relay. Keep
prefixes intact; media bodies never traverse the API process. No Funnel/public
exposure, exit-node configuration, per-provider public backend or extra default
VPN container. This is the target design, not deployed infrastructure.

Source egress is isolated from the internal control listener. Keep private API
control communications reachable without granting private-IP exceptions to media
fetches. API/catalog must not be forced through Mullvad. Limit NET_ADMIN to tunnel/
firewall setup; providers and relay run as reduced-privilege processes.

## Health

- Private `/health/livez`: process loop/task supervisor alive; no upstream URL
  fetches. A database/provider outage must not trigger a whole-container restart.
- Private `/health/readyz`: relay store/key readiness, bound controlled transport,
  current VPN epoch and admission/drain state. No sensitive endpoint/IP/key data.
- Streaming-stack aggregate **new playback** readiness also checks provider health.
  Existing relay sessions remain routable when only providers are down; do not
  remove the relay route merely because new resolution is unavailable.
- Metrics on private control networking only, bounded labels/histograms and output.

Measure stage timings and safe categories: auth, store lookup, admission wait,
DNS/connect/TLS/headers, manifest parse/rewrite, upstream first byte, streamed
bytes, cancellations, range mismatches, idle timeouts, resource limits, source
expiry, generation switches and VPN epoch transitions. Do not label metrics by
upstream URLs, query strings, session/resource IDs, profile/user names or JWTs.
Never log arbitrary wrapped HTTP-client errors that may embed signed destinations.

## Activation, drain and recovery

Version provider and relay artifacts independently. Supervisor drains/replaces
only the requested component and restarts its failed process, not healthy peers.
Provider updates leave relay/VPN running; relay updates leave provider/VPN running.
A whole streaming-container image replacement remains a distinct disruptive path.

SIGTERM stops new session admission, reports draining, allows bounded active
request drain, then cancels remaining streams, joins tasks and flushes durable
state. Do not wait indefinitely for multi-hour MP4 bodies. Healthy ordinary
requests must not require a per-segment provider/API/catalog dependency.

Reopen relay-owned encrypted metadata on restart, validate its version/keys, then
reattest VPN before serving. Existing resource IDs stay stable. A lost process/TCP
connection is not magically preserved; players may retry/reload with valid grants.
Refresh expired or new-exit-IP-bound sources through API/provider coordination.
State-encryption key rotation retains old read keys through migration/rollback;
never destroy keys needed to recover persisted sessions.

Store schema migrations must be versioned, transactional, backup-aware and
compatible with a documented rollback procedure. Refuse unknown/newer formats
rather than resetting state. Media is never backed up or spooled. Sessions,
revocations and idempotency maps have bounded expiry cleanup.

## Local implementation and future packaging

Build and run locally on the Mac when explicitly authorized, using its Mullvad
connection and interface-bound DNS/sockets. Use existing saved Go selections as
private input to the real relay control importer. Do not require SSH/share access,
production deployment, system firewall changes or privileged commands to develop.

Container files and supervisor/firewall packaging are future implementation tasks;
this repository setup does not supply an unverified production Compose file.
When deployment is authorized, verify source egress from the running streaming
namespace, storage persistence, control isolation, component restarts and actual
VPN loss. A successful startup/build or file mount is not acceptance evidence.

## Stage-6 process activation and rollback contract

Packaging remains **deferred until separately requested**. The procedures below
specify responsibilities; they are not a supplied supervisor, container image,
firewall installation or deployment result.

The streaming supervisor owns independent provider and relay artifact versions,
process health, bounded restart backoff and the VPN/firewall lifecycle. Stage an
artifact under a versioned path, validate its configuration/schema/key compatibility,
then stop admission and signal only the requested process. Wait its bounded drain
and joined exit before atomically selecting the staged artifact for the next start.
Verify private component health before accepting new playback setup. Retain the
previous artifact and consistent metadata backup through the rollback window.
Provider replacement does not signal relay or VPN; relay replacement does not
signal provider or VPN. Avoid a container-wide restart for a single component's
failure. Aggregate new-playback health combines provider and relay state outside
the relay, while existing relay sessions retain their own routing/health policy.

Relay shutdown order is explicit in the daemon owner: mark draining and stop
listener admission; HTTP/1 connections receive graceful shutdown and may finish
for at most lifecycle.drain_timeout_s; then abort/join remaining connection tasks,
including clients that never poll/read again. The transport guard monitor and
connection drivers, plus the store worker and revocation leases, stay running
through this interval. Next cancel and join transport drivers/monitor; finally
stop the store worker, discard queued calls and close SQLite after its in-flight
transaction has settled. Store failure cancels egress immediately; transport/task
failure marks liveness/readiness failed and initiates bounded listener shutdown.
Do not use health polling that fetches media or tie relay liveness to providers.

After exit, activate or roll back the component artifact and reopen the existing
private state directory with its separately provisioned keys. Startup authenticates
all retained records, checks SQLite/schema/count bounds and rejects unknown versions
or missing keys. Controlled egress must establish a fresh epoch before source
traffic/readiness; persisted source context is not attestation. Clients reconnect
or reload with unexpired grants and may need source refresh after expiry or an
exit-IP change. TCP connections do not survive process replacement. Whole-container
replacement also restarts VPN/providers and is disruptive; no uninterrupted-playback
claim is made.

Take a consistent SQLite metadata backup after joined shutdown (or use an approved
SQLite backup mechanism with appropriate isolation); do not copy a changing main
file without its transaction state. Retain encryption keys separately, including
all historical read keys needed by the database/sentinel/backups. Never log keys or
copy them into state or Git. Stage 6 leaves schema version 1 and blob/AAD formats
unchanged, so artifact rollback to stage 5 needs no database downgrade, but removes
stage-6 recovery/health behavior. Restore a known matching artifact/database pair
when formats change; never erase/reinitialize state as recovery. Revocations made
after a backup must not be undone by restoration: reconcile them from a trusted
authorization record or invalidate affected grants/sessions before admitting traffic.

Future schema/encryption migrations must use an explicit transactional version
upgrade, authenticate records with retained old keys, prove bounded migration and
rollback against a consistent backup, and reject newer schemas in older binaries.
Do not remove old keys until migration, backup retention and rollback windows have
all completed. Merely changing the active write key is not bulk re-encryption.

## Applied diagnostics

Readiness returns runtime component state, independently of overall acceptance
(which remains false). Store/key health means successful startup authentication
and a current bounded worker/database/free-space check, not filesystem discovery.
Admission is a snapshot of control/transport capacity, not a viewer capacity claim.
Private health and metrics require the control bearer; unavailable/poisoned stores,
stale VPN epochs, owned task failure and drain all prevent runtime readiness.
During drain listeners close; readiness may become unreachable after admission
stops, which is an expected shutdown signal.

Metrics use fewer than 64 fixed series and 8192 output bytes. Counts and microsecond
sums cover authorization, store, preparation, delivery preparation, fetch, admission,
DNS, exchange, connect, TLS, response headers, time from response headers to first nonempty transport
frame, and manifest parse/publication.
Timings include canceled/failed scopes and may overlap; they are not percentiles
or a tracing system. Upstream bytes count accepted transport data frames once
(including DoH data but not prefix replay or headers); delivered bytes count frames yielded by pull media
bodies, not TCP acknowledgments. Manifest/key bodies are bounded separately and
are not included in that media-yield counter. Cancellation counts include unfinished
body drops; a failure may be observed at more than one boundary. Safe failure
buckets distinguish upstream, timeout, admission, state, cancellation, range,
expiry, authorization, manifest, VPN and refresh exhaustion. Generation commits,
VPN transitions and forced drain deadlines have counters. No label or output uses
source/provider hostnames, URLs, credentials, grants, user/session/resource IDs or
arbitrary error strings. Counters reset on restart and saturate rather than wrap.

Private diagnostics have one independent admission permit, so a readiness request
does not consume control capacity and remains observable under control saturation.
Excess concurrent diagnostic requests fail with request_limit. HLS recovery reserves
auth.max_grant_bytes in every rewritten URI before activation; an output that cannot
fit max_playlist_bytes returns manifest_limit without changing the active generation.
