# Authorization, durable sessions and recovery

## Playback grants

The Go API signs JWTs using Ed25519; the relay has only provisioned public keys,
indexed by a bounded `kid` allowlist. Pin EdDSA, issuer `family-stream-api`, audience
`family-stream-relay`, and required `sub`, `sid`, `arv`, `iat`, `nbf`, `exp`, `jti`.
`sub` is the opaque owner ID; `sid` is the session ID; `arv` is the session's auth
revision. Reject `none`, wrong algorithms, unknown keys, missing claims, oversized
JWTs, duplicate claims, invalid timestamps and expired/not-yet-valid tokens.
Permit at most 30 seconds of configured clock leeway; never use leeway to extend
source leases. Keys are loaded from operator files, not arbitrary JWT URLs.

Default session maximum and maximum grant lifetime are six hours, bounded by the
stored expiry. This is a configurable authorization lease, not a claim that a
source stays available that long. API should issue a grant valid for its planned
session within that cap, so VOD child URLs do not routinely expire mid-film.
Short grants are allowed but require an actual client renewal/reload strategy.
Do not promise transparent renewal of already-issued VOD segment URLs.

Compare owner, SID and auth revision against local durable state on every stream
request. Grants work during API/provider/catalog outages until their own lease,
revocation or source validity stops them. Auth revision starts at 1 and is returned to the API for grant issuance; it is
separate from the control CAS revision and does not change on source switches or
lease renewal. Revocation increments it and persists a tombstone, invalidating old
grants without a network call. A revoked session cannot be renewed or reactivated;
new authorization requires a new session. Revocation cancels affected active requests.
Grant/session expiry bounds active responses as well as new requests.

`st` query grants are required for native media URL compatibility. Redact path
resource IDs and all query strings in access logs; use Referrer-Policy no-referrer
and private/no-store. Grant-bearing URLs must not appear in analytical events,
errors, redirect targets outside the relay or persistent raw request logs.

## Data model

- Session: random >=128-bit ID, opaque owner, contract/store versions, expiry,
  revision, auth revision, lifecycle state, active source generation, encryption
  key ID and a random per-session resource-ID secret.
- Source snapshot: selected + ordered backups, exact title/cross-IDs/coordinates,
  fetch context, optional source expiry, HLS selected child/group association,
  supported format and bounded private metadata. No provider call on a segment.
- Resource registration: session/generation, opaque >=128-bit ID, encrypted full
  resolved upstream URI, resource kind, context fingerprint, optional range
  constraint and retention/expiry. IDs never encode upstream hosts or URLs.
- Revocation tombstones and idempotency entries: bounded, durable, expiry-indexed.

SQLite belongs only to the relay. Use authenticated application-level encryption
for sensitive snapshot/resource blobs (AES-256-GCM with unique random nonces,
versioned key IDs and row/session/generation/type as associated data). The master
key comes from a read-only operator secret; do not store it beside the database.
SQLite is not encrypted merely because blob fields are encrypted: keep sensitive
URLs/context out of indexes, plaintext columns, WAL/logs, temporary files and SQL
traces. Encrypt or avoid sensitive idempotency payloads too.

Persist registrations in a batch transaction before emitting rewritten URIs.
Use a bounded hot resource cache with database fallback. Resource reads must not
write a heartbeat on every segment; batch coarse last-use updates. Session cleanup
runs as an owned periodic task with bounded batches. Never evict active resources
to admit new work; reject admission first. Drain active readers before deletion.

## State and generation transitions

CREATING -> ACTIVE -> DRAINING -> EXPIRED, or ACTIVE -> REVOKED. Store failure
before publication leaves no usable URL. Crash recovery ignores incomplete
CREATING records; startup validates schema, decryption keys, leases and records.
Corrupt/unreadable records fail closed with a safe category.

Every source change increments `revision` and creates a new `source_generation`.
Never reuse an old resource ID for new bytes. Old generation resources can remain
for a bounded grace/drain interval, bound to their original sources; they must
never redirect into the new generation. Failover means a new playback item/root
and caller-managed resume, not hiding a new timeline behind an old segment URI.

Renew accepts expected revision and a requested new session expiry within policy.
The API signs a new grant using returned state; existing grants expire as issued.
Extending a session does not extend provider URLs. Source refresh separately
imports a matching new selection. Future cross-service clients must explicitly
handle replacing the playback item and restoring media time if old URLs expire.

## Restart and release boundaries

Provider process restart: imported sessions/resources remain available without
provider calls. Relay restart: restore encrypted metadata and stable resource IDs,
then require healthy guarded egress before readiness. Open TCP connections do not
survive a process replacement; retry/reload may resume via restored URLs if grants
and sources remain valid. VPN exit-IP changes may invalidate upstream IP-bound
sources; restoration is not proof of source validity.

Component rollout first stops new session admission, drains active requests up to
a configured deadline, flushes durable state, then replaces the process. Full
streaming-container replacement also restarts VPN/providers and breaks active
connections. Seamless hot handoff is a separate proven mechanism, not guaranteed
by persistent state or signed grants. Unknown store versions fail startup instead
of overwriting prior state. Back up metadata and retain required decryption keys
for a documented rollback window; never back up media bodies.

Stage 6 keeps SQLite schema version 1 and its encrypted blob/AAD layouts. Prepared
source activation extends the existing source transaction with its bounded resource
batch. No new recovery cursor, table or unbounded retry log is persisted. Successful
failover stores the remaining backup suffix. Restored revocations, generation IDs,
resource mappings and CAS revisions use the same startup validation. Failed
preparation/publication changes none of those records. See stage-6 evidence for
offline restart proof and operations for backup/migration/rollback requirements.
