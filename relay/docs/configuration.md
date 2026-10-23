# Configuration and limits

`config/relay.example.toml` is a non-secret typed configuration example. The stage-1
parser validates startup/limit invariants; see [stage-2 coverage](stage-2.md). No production `unsafe`, unbound-interface,
allow-private-upstream or unauthenticated-control switch may be added.

## Units and policy

Use `_bytes`, `_ms`, `_s`, `_count` names and checked conversions. Reject zero,
negative, overflowing or inconsistent settings. Suggested defaults below are
resource guards for implementation/verification, **not measured capacity claims**.
They may need adjustment from authorized evidence; never advertise supported
users/concurrent viewers from these values.

| Setting | Initial policy |
| --- | --- |
| Local control / stream listeners | 127.0.0.1:8091 / 127.0.0.1:8092 |
| Session and grant maximum | 21,600 seconds; known upstream expiry remains separate |
| JWT clock skew / maximum token | 30 seconds / 4,096 bytes |
| Control request body | 4 MiB; reject before parsing beyond cap |
| Retained sources/session | 128 total, selected + at most 127 backups; oversized input rejected |
| Concurrent control requests / store queue | 4 / 32; finite admission wait |
| Admitted sessions | 128; independent of verified streaming capacity |
| Resource registrations/session / total | 20,000 / 100,000, including retained generations |
| Resource hot index budget | 16 MiB, bounded database fallback |
| Persisted session snapshot | 2 MiB; source/private-metadata limits checked explicitly |
| Playlist body / nested playlist depth | 1 MiB / 6; reject cycles and excessive graphs |
| Key body / sniff prefix | 4 KiB / 4 KiB; AES-128 identity key itself must have expected length |
| Global / session / authority upstream streams | 32 / 8 / 8 |
| Request admission wait | 2 seconds; reject rather than queue indefinitely |
| Application body chunk | 64 KiB; account for additional library/frame buffers |
| DNS / connect / TLS / response-header waits | 3 / 3 / 3 / 8 seconds |
| Upstream read idle / downstream write idle | 20 / 30 seconds |
| Playlist total / segment total | 20 / 120 seconds, clipped by authorization/session expiry |
| MP4 absolute transfer maximum | 21,600 seconds, clipped by grant/session expiry; no full-body short provider timeout |
| Redirects | 3 maximum; no downgrade or POST replay |
| Retained old generation grace / drain deadline | 120 / 30 seconds |
| Cleanup batch / sweep period | 128 records / 30 seconds; no per-segment sweeps |

Bound transport header bytes/count, decompressed sizes (if ever explicitly added),
connection pool entries, body frames, blocking store workers and pending futures
too. Compute memory accounting across control payloads, manifests, prefix buffers,
stream chunks and hot caches; individual caps do not prove an aggregate bound.

Store disk space/free-space reserve and metadata retention need hard admission
limits. No media bytes, decryptor captures or body caches are persisted. A full or
unavailable store returns a safe typed error and stops publishing new resource IDs.
Never drop live resources silently to fit a new session.

## Startup validation

Require secret-file permissions and usable control bearer, API public verification
keys, session encryption key, supported platform guard implementation, distinct
private listener addresses, and a writable relay-owned state directory. Validate
configured public path prefix `/stream`, allowed grant issuer/audience/algorithm,
JWT/session lease relationships and cross-limit relationships.

Wait for guard readiness before any source socket. Failure to bind/attest Mullvad
is a readiness failure, never a direct-connect fallback. Local development needs
no Server or container deployment. Configuration attestation is not runtime proof.

Secret values are never configuration examples. File paths are operator inputs;
state and actual secrets stay ignored, outside Git and client/control responses.
Provisioning system keychains, firewall rules, schedules or VPN state requires
separate explicit authorization.

Stage 2 key-file format, platform limitations and applied transport bounds are
documented in [stage-2 evidence](stage-2.md). Verification keys are now mandatory
at daemon startup; stage-3 encrypted-store startup now validates encryption keys, permissions, schema,
record authentication, state bounds and database access. See [stage 3](stage-3.md).

## Stage-3 store settings and secrets

`state.max_database_bytes` defaults to 1 GiB (allowed 1 MiB–64 GiB). SQLite's page
quota caps the main database; the DELETE journal has an additional bounded budget
up to the main database size. `min_free_space_bytes` defaults to 64 MiB, with extra
transaction headroom reserved at admission. `max_idempotency_count` defaults to
256 (allowed 1–65,536). Session/resource count and snapshot limits still apply;
these settings are admission guards, not measured capacity.

Provision `state.encryption_key_file` as a mode-0600 regular file, outside the
state directory and Git. It contains JSON with `active_key_id` and `keys`: each
key has `kid` (16–128 opaque ASCII characters) and `key` (a base64url-no-padding
32-byte AES key). At most eight distinct IDs are accepted. Use an OS cryptographic
random generator; do not derive the key from the bearer, owner or a password.
All writes use the active ID; historical keys must remain for existing rows and
backups, including the schema sentinel. Removing a needed key fails startup.
Rotation here supports retained read keys/new writes, not bulk re-encryption.

The relay creates a missing state directory as 0700 and files as 0600. Existing
state directories must have no group/world permission. Keep secret files in a
separate private operator directory; do not copy keys into database backups.
Restore the consistent metadata backup together with separately provisioned
historical keys. Unknown schema versions are rejected; no downgrade or automatic
rewrite of existing records is performed.

Stage 4 adds `limits.max_manifest_registrations_count`: default 4096, allowed
1–20,000. This independently bounds a single atomic manifest registration batch;
cleanup_batch_count continues to bound cleanup work. Aggregate serialized
registrations are bounded during parsing by state.max_snapshot_bytes, before
context copies can accumulate beyond that budget. Playlist input and rewritten
output each obey max_playlist_bytes; depth includes original/final redirect
ancestors. Existing total/per-session state and transport/frame limits also apply.

## Stage-6 applied recovery and health policy

Recovery reuses the existing control, transport, manifest/state and generation
limits; it adds no second client or unbounded recovery queue. The explicit
operation deadline is eight seconds (inside the ten-second control boundary),
with a five-second candidate expiry margin. Manifest input is an aggregate
max_playlist_bytes budget across the operation. MP4 prefix evidence and bounded
read-ahead are described in failover.md. Runtime readiness waits at most 500 ms
for the actual store owner; no upstream media is fetched. Metrics have a fixed
vocabulary, fewer than 64 series and 8192 bytes, independent of input cardinality.
Drain uses lifecycle.drain_timeout_s (30 s default); the transport/store remain
running through listener drain, then transport drivers join before store shutdown.

Private diagnostics have one independent admission permit, so a readiness request
does not consume control capacity and remains observable under control saturation.
Excess concurrent diagnostic requests fail with request_limit. HLS recovery reserves
auth.max_grant_bytes in every rewritten URI before activation; an output that cannot
fit max_playlist_bytes returns manifest_limit without changing the active generation.

Linux operations packaging supplies a root-owned, short-lived
`/run/family-stream-vpn/attestation.json`; missing/stale state fails closed and
invalidates the transport epoch. All source and DoH sockets bind to `wg0` using
`SO_BINDTODEVICE`, with only `CAP_NET_RAW` retained by the relay process. The
privileged VPN owner and namespace firewall are supplied by the separate deploy
repository. `http.trusted_listen_ip` optionally selects one literal private IPv4
address for listeners on an isolated same-host bridge. Control bearer and signed
stream grants remain mandatory; no upstream private-IP exceptions are added.
Linux packaging builds are separate from runtime VPN-failure acceptance.
