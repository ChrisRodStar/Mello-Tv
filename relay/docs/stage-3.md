# Stage 3: durable sessions, resource identity and lifecycle

Merged in [PR #3](https://github.com/ChrisRodStar/family-stream-relay/pull/3)
as `c9345366af4e1d6906e83ec87bb0dbc549d7902b` on 2026-10-15.

Implemented locally on macOS arm64, 2026-10-15, from main `1f3e700` (includes
stage-2 merge `5d0d9e4485c4fc95b466695322482207e72b88ab`). No provider/media
requests, Server access, deployments or system VPN/firewall changes were used.

## Implemented behavior

The daemon opens the relay-owned store before accepting HTTP work. Schema v1
creation and its encrypted key-check sentinel commit atomically. Unknown schema
versions, missing historical keys, failed authenticated decryption, invalid
associations, excessive retained state and SQLite integrity failures fail startup.
An exclusive sidecar lock permits one relay owner per database. State directories
must be private (0700); database files must have no group/world permissions.
New directories/files are created 0700/0600. Existing permissions are validated.

Sensitive session authorization fields, the per-session identity secret, complete
provider snapshots, resource URI/context/range/type/expiry, and idempotency
fingerprints/replies use AES-256-GCM. Each row envelope has version 1, a key ID,
a random 96-bit OS-generated nonce and an authentication tag. AAD binds the store
version, key ID, row category, session, generation and resource identifier as
applicable. URLs/headers/owner IDs are absent from plaintext SQL columns and
indexes. The encrypted payload includes the resource kind; its identity also
incorporates the complete context (a context fingerprint), URI, range and expiry.
This is application-level encryption, not full-database encryption: opaque IDs,
generation numbers, expiry indexes, ciphertext lengths and the schema are visible.
No SQL tracing or media persistence is enabled.

`POST /internal/v1/sessions` now commits the ACTIVE session, generation snapshot,
root registration and owner-scoped idempotency result in one transaction before
201. There is no externally visible intermediate CREATING record. A failed or
interrupted transaction publishes no usable mapping. Equivalent parsed JSON and
UTC timestamp spellings share the same request fingerprint; conflicting payloads
return 409. Replays return the original redacted result until its original session
lease expires, including after mutation/revocation; authorization always consults
current durable state. Expiry responses use whole Unix seconds consistently.
Idempotency entries expire at the original create lease and have a separate hard
count cap; expired entries become reusable after bounded cleanup.

Authenticated GET returns redacted state. Renewal checks owner/CAS, extends only
the bounded session lease and increments control revision. Source replacement
checks every known title/cross-ID across both selections, validates and bounds the
new snapshot, and transactionally advances revision/generation/root while keeping
auth revision unchanged. Known cross-IDs are also retained in the encrypted session identity so an
intervening selection that omits them cannot permit a conflicting later refresh.
It returns `resume_required: true`. Old resource IDs
continue to name only their original bytes for the bounded grace interval.
Renewal never extends a known upstream expiry. DELETE idempotently persists the
revoked session as a tombstone, advances auth/control revisions once and cancels
active reader tokens. Revoked sessions cannot renew or change sources. Tombstones
remain until session expiry and drain; signed grants cannot resurrect them.

Resource IDs are 256-bit HMAC-SHA256 values using a random encrypted per-session
secret and generation-bound identity. Registration batches deduplicate stable
identities and commit all rows before returning IDs. Active retained resources
are never evicted for admission. A byte-accounted FIFO cache holds serialized
resource metadata, with SQLite fallback and no per-read heartbeat writes.
Authorization checks the session and generation snapshot plus registered resource
ownership for both GET/HEAD and every resource kind. Reader guards prevent cleanup
from deleting mappings still held by an authorized response; cancellation and the
returned minimum grant/session/source/resource/generation deadline must remain
attached to future stage-4/5 response bodies.

One owned blocking worker serializes database access through the finite store
queue. Control concurrency/body caps and pre-queue snapshot/batch limits bound
pending work. SQLite uses FULL synchronous commits, DELETE rollback journals,
foreign keys, an in-memory temporary store and a bounded page cache. The configured
page quota caps the main database; a rollback journal can additionally reach that
quota (roughly twice the database budget overall). Admission also checks the free
space reserve plus conservative transaction headroom. Quota/I/O/decryption failures
poison the worker, cancel reader tokens and return `state_store_unavailable`;
restart/recovery is required before further publication.

The worker owns periodic cleanup and expiry cancellation. It pages generation
scans and resource deletion, then removes empty generations/expired sessions and
expired idempotency entries in bounded batches. Partial expired/grace-drained
records are valid across restart. No live resource deletion occurs while readers
hold guards. There are no per-segment state writes. At shutdown, the current
operation settles; queued operations are discarded safely, admission closes,
readers cancel and the worker/connection are joined. A timed-out caller may retry
a committed create via idempotency. No detached store task is introduced.

## Offline evidence

Required checks:

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
```

All three required commands passed. The suite has **65 passing tests**, including
all 37 stage-2 regressions, a legacy-configuration regression, two store queue/budget tests and
25 session integration tests (one is a subprocess crash-fixture entry point).
Coverage includes committed restart recovery, real hot-journal rollback after
abrupt process exit, same IDs after restart, owner-scoped idempotency/conflicts,
concurrent CAS winners, title/cross-ID rejection, source-generation grace and
reader drain, lease renewal/source expiry, durable revocation, batch/global/session
limits, database fallback without read writes, SQLite-full/transaction rollback,
wrong/missing/rotated keys, ciphertext bit flips/row substitution, unknown schema,
single ownership and joined shutdown. Real loopback `relayctl` now gets 201;
GET/HEAD grant authorization uses the real encrypted store in router tests.

See [dependency audit](dependency-audit.md) for stable-release verification and
the committed Cargo.lock. These tests do not establish sustained media streaming,
physical power-loss/storage-hardware behavior, decoder/player/device playback,
source availability after restart or uninterrupted TCP connections.

## Explicit remaining gates

Overall readiness stays false. Successful durable authorization returns 503
`media_unavailable`; HLS/MP4 delivery is not implemented. `/failover` returns 503
`failover_unavailable` until stage 6. The production guarded transport remains
the only future upstream boundary and is unchanged here. Full source validation
still occurs at connection establishment. No source/network availability is
inferred from metadata recovery, grants or an imported selection.

Stage 4 is a separate implementation task; merging this stage does not establish
any HLS/media-delivery acceptance item.

## PR review follow-up

Greptile reviewed `bd20bce5f6b0e10a3bfd8d36ed2f2dfdf9d0749f` in PR #3 on
2026-10-15 and raised two valid findings. New budget settings now have the
documented Serde defaults for existing configurations (explicit invalid values
still fail validation). Create/source admission now counts retained generation
rows as well as resources, using the same per-session/global bounds as restore.
A regression exercises the reported two-resource/one-record-cleanup case,
rejection without mutation, and successful restart before/after later admission.
No review findings were disputed. Inspection also added durable known-cross-ID
retention across omitted metadata and restart, plus an explicit global resource
budget regression. All required checks were rerun on the follow-up changes.

Greptile completed its follow-up review of
`4bc50212692564ad55458fa067768e9b910eb817` at 2026-10-15 11:58:07 UTC:
26 files reviewed, zero new comments, successful check. Both original threads
were resolved before merge.
