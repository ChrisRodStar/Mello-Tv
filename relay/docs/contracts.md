# relay.v1 HTTP contract

This is the relay implementation contract; API adoption is separate. Its
machine-readable definition is [OpenAPI](../contracts/openapi.json). The Go API
project must adopt this private boundary during integration; providers.v1 remains
a separate existing wire model.

## Private control listener

Every `/internal/v1/*` request requires a dedicated service bearer loaded from a
secret file, constant-time checked and never accepted as a playback grant. Bind
only loopback locally/private networking in deployment. Do not route it through
Tailscale Serve. Deployment transport uses an authenticated private channel;
if control traffic leaves the host's isolated container network, require mTLS.
Never send this bearer on upstream media requests.

| Method/path | Behavior |
| --- | --- |
| POST `/internal/v1/sessions` | Import one providers.v1 selection, persist session/root registration, return relative entry path |
| GET `/internal/v1/sessions/{session_id}` | Redacted state, source generation and safe issue codes only |
| DELETE `/internal/v1/sessions/{session_id}` | Idempotently revoke, persist tombstone, cancel active tasks; 204 |
| POST `/internal/v1/sessions/{session_id}/renew` | Extend bounded session lease with expected revision; API must issue a new grant |
| POST `/internal/v1/sessions/{session_id}/source` | Replace sources from a fresh matching providers.v1 selection using compare-and-swap |
| POST `/internal/v1/sessions/{session_id}/backups` | Add completed verified backups with owner and expected generation; 204; keep playing source, revision and grants unchanged |
| POST `/internal/v1/sessions/{session_id}/failover` | Activate the next valid stored backup in provider order; create a new generation |

POST create takes `contract_version: relay.v1`, `owner_id` (opaque), `expires_at`,
and the full `selection` envelope. `Idempotency-Key` is required: retrying the same
owner/key and normalized payload returns the original result; a different payload
under that key returns 409. Commit the durable session/resource mappings before
201. Admission limits return 429/503 before allocating an unbounded queue.

Control replies contain `session_id`, relative `entry_path`, observed `format`,
`expires_at`, `revision`, `auth_revision`, `source_generation`, and `state`.
The API uses returned `auth_revision` as the signed grant's `arv`; it must not
substitute the independently changing control revision. No reply contains
upstream URLs, raw provider metadata, cookies, grants, keys or signed context.
API owns grant issuance, user-visible playback coordination and progress/resume.
GET control state can report safe counts and errors, never snapshot internals.

Renew/source/failover take an `expected_revision`; stale mutations return 409.
Owner/session association is checked by the authenticated API before a mutation;
relay matches owner IDs supplied in create/renew/source/failover bodies and
enforces CAS semantics. GET and DELETE are privileged service-control operations;
the API must authorize their owner before calling them. State transitions and generation
activation must be transactional. Deletion is safe to repeat but does not reveal
other owners' source state. There is no arbitrary-URL fetch endpoint.

## Authorized streaming listener

- GET/HEAD `/stream/{session_id}/entry?st={grant}` serves the active generation's
  entry resource without an upstream redirect. MIME identifies HLS versus MP4.
- GET/HEAD `/stream/{session_id}/r/{resource_id}?st={grant}` serves only a resource
  durably registered to that session and generation.
- `st` is an Ed25519-signed JWT scoped to this session/owner/auth revision; verify
  it on **every** request, including HEAD, key, subtitle and byte-range requests.
  It is a capability for registered session resources, never an upstream URL.
- Native players follow URLs and may not propagate custom headers. Rewrite the
  grant onto each same-origin child URI. Do not require cookies or custom player
  headers for correctness. Never forward `st` to upstream, external redirects,
  unrelated hosts or telemetry. Reject duplicate credential/query parameters.
- Relative output paths preserve the `/stream` prefix. API exposes one private
  client address through Serve; no client sees a provider/media host.

GET and HEAD have the same authorization/status/representation semantics; HEAD
sends no body. Range semantics and supported headers are in `docs/media.md`.
Unsupported methods return 405 with Allow, and never trigger upstream work.
Health routes are private and bypass stream token validation only on that private
listener, not as a public proxy exemption.

## Safe failures

Control and pre-body stream errors use `application/problem+json`, with stable
`code`, safe `detail`, request ID and `retryable`. Never include a request URL,
upstream URL/header/body, JWT or internal filesystem path. After media headers/
bytes are sent, terminate the affected body on error; never append JSON to video.

| Status | Stable examples |
| --- | --- |
| 400 | invalid_range_syntax, duplicate_grant, invalid_input |
| 401 / 403 | invalid_grant, expired_grant / revoked_grant, owner_mismatch |
| 404 / 410 | unknown_resource / session_expired, source_expired |
| 409 | revision_conflict, resume_required, source_refresh_required |
| 413 / 415 / 422 | import_too_large / unsupported_format / invalid_selection |
| 416 | range_not_satisfiable; valid `Content-Range: bytes */length` when known |
| 429 / 503 | request_limit / draining, vpn_unavailable, state_store_unavailable |
| 502 / 504 | upstream_rejected, invalid_manifest, range_mismatch / upstream_timeout |

A relay transport error does not establish provider resolution failure, wrong
language or media content identity. Keep safe stage/cause codes distinct.

## Stage-6 operational routes

`/health/livez`, `/health/readyz` and `/metrics` require the same private control
bearer as mutations and are never Serve routes. Liveness reports owner failure;
readiness performs a bounded store-owner round trip and inspects current guard,
verifier, admission and drain state without fetching upstream media. Readiness
includes `store_keys`, `transport_epoch`, `admission`, `owned_tasks`,
`provider_readiness: external` and `overall_acceptance: false`. An absent provider
process is not an existing-session failure. The supervisor owns aggregate new-playback
readiness; it must not withdraw usable relay routes on provider failure alone.

Metrics are Prometheus text with a fixed vocabulary (under 64 series/8192 bytes),
process-local saturating counters and stage count/microsecond sums. They contain
no arbitrary request/source labels or identifiers. See operations for semantics.
Failover/source replies use SessionReady with resume_required true. Stale revisions
are 409 revision_conflict; inspect GET status after lost replies before intentionally
requesting another switch. See failover for preparation budgets and retry behavior.

Private diagnostics have one independent admission permit, so a readiness request
does not consume control capacity and remains observable under control saturation.
Excess concurrent diagnostic requests fail with request_limit. HLS recovery reserves
auth.max_grant_bytes in every rewritten URI before activation; an output that cannot
fit max_playlist_bytes returns manifest_limit without changing the active generation.

## Startup backup enrichment

An opted-in provider startup reply may have `pending: true`. The API can import
that verified selection immediately, then own a bounded completion request.
`POST /internal/v1/sessions/{session_id}/backups` takes `owner_id`,
`expected_generation` and a complete matching `selection`. The store validates
all sources and identity, serializes against revocation/source changes, deduplicates
source IDs, and bounds the resulting snapshot. It keeps the active selected source,
root, generation, control revision and auth revision unchanged. Repeating the same
metadata is safe; a stale generation is rejected. No playback switch, new grant,
upstream fetch or resource registration occurs until actual failover.
