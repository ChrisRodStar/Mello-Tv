# Source failure, ordered backups and refresh

Provider selection supplies the active source and backup order. The relay never
re-ranks by provider name, advertising, protocol family, source score or presumed
language. Availability is per source/rendition at a particular time, not a global
provider quality claim.

## Safe activation

A session stores source snapshots and independent source generations. Validate
format, title/episode, known expiry, FetchContext and selected-master linkage
before activating a source. Source preparation uses the same guarded transport,
bounded manifest/header work and durable registration as ordinary delivery.

Entry-time fallback is deliberately not implemented. GET and HEAD always use the
active generation, including the first request and concurrent requests after
restart. There is no exposure bit or unpublished fallback cursor. Only an explicit
authenticated failover/source operation changes the generation. A failed resource
never tries another source or becomes an alias for the replacement's bytes.

A failure after body bytes are committed terminates that response. Before body
commit, return a safe typed status such as upstream_rejected, source_expired,
vpn_unavailable or invalid_manifest. Distinguish timeout, admission/byte budget,
unsupported codec/protection and actual upstream rejection in metrics/control
state. A failed probe or exhausted local budget is not proof that a provider is
universally unavailable.

## Explicit control operation

`POST /internal/v1/sessions/{sid}/failover` requires stored owner ID and expected
revision. It advances to the next unexpired, supported candidate in the original
order, prepares the new entry, and transactionally publishes a new generation.
Control returns `resume_required: true`, new revision/generation/format and the
relative entry path. Clients/coordinator reload a new playback item and resume
at their recorded position. Time alignment/language between providers is not
guaranteed; preserve explicit metadata and never claim seamless switching.

Concurrent failover requests use CAS and idempotent state transitions; losers
receive revision_conflict; GET the redacted session status to resolve the outcome. Old generation IDs remain
bound to their old source for bounded in-flight/grace retention only. They never
become aliases for the new generation.

If no stored source remains usable, return source_refresh_required. The API can
ask providers for a new selection using the stored known title IDs/coordinates,
then call the source-update endpoint with that selection and expected revision.
This is a setup/recovery operation, not a synchronous per-segment dependency.
The relay does not call TMDB/catalog or run provider adapters.

## Failure boundaries

- Provider/API/catalog outage: existing authorized, unexpired resource requests
  continue from durable snapshots; refreshing/renewing may be unavailable.
- VPN loss: cancel source egress and invalidate pooled sockets. Do not try another
  provider over an ordinary route. Reattestation is required before preparation.
- VPN exit change: an IP-bound source may reject requests. Treat this as source
  validity loss and coordinate refresh, not a reason to loosen validation.
- Relay/state-store loss: readiness and new work fail closed; recover metadata
  first. Do not create fresh mappings under the same IDs from guessed URLs.
- Unsupported media protection/format: reject explicitly, preserve evidence;
  do not label encrypted bytes as clear or invent a transcoding fallback.

Offline verification must assert that a failed segment on source A never causes
source B's bytes to be served at A's resource ID. Live switching verification must
record player reload/resume and any actual discontinuity separately from byte
accessibility or a successfully issued control response.

## Applied recovery limits and retries (stage 6)

Both control operations obtain a consistent stored owner/revision/identity snapshot,
release the store worker, prepare with the sole guarded transport, then call the
existing encrypted source transaction under CAS. The same transaction commits the
new session, old-generation grace, new root and all prepared HLS graph mappings.
Preparation does not persist media or partially prepared generations. No store or
session lock spans network work. Publication checks cancellation, deadline and the
preparation's VPN epoch again immediately before commit.

Failover attempts at most the stored backup count (127 by default), in array order,
never a provider/score sort. It skips sources expiring within five seconds. Each
operation has an eight-second preparation/publication deadline, also clipped by
session and candidate expiry, inside the existing ten-second control boundary.
All attempts share max_playlist_bytes (1 MiB default) of accepted manifest input;
an over-budget frame fails preparation_limit and is dropped. MP4 uses the shared
HEAD delivery preparation: a GET with 16-byte BMFF prefix validation, at most one
bounded pending transport frame, then closure. It reserves 16 evidence bytes from
the operation budget per candidate; transport-frame/attempt limits separately bound
read-ahead. No MP4 body is collected. HLS follows all retained manifest/rendition
references with existing depth/cycle/URI/redaction checks. A single combined batch
obeys max_manifest_registrations_count and max_snapshot_bytes, and publication
obeys total/per-session resource, generation and database admission limits.

Source-specific upstream rejection, invalid/unsupported manifests and representation
mismatches may advance to the next backup. Exhaustion or only expired alternatives
returns source_refresh_required. VPN loss, timeout, cancellation, admission, byte,
manifest-output and state/store limits stop immediately with their own safe category;
they do not prove exhaustion. Source refresh prepares only its supplied selected
source and retains its supplied backup order. API/providers obtain fresh selections;
there is no relay provider endpoint, ranking, or per-segment service dependency.

Failed preparation leaves the selection/revision/generation unchanged, including
its backups. Retries repeat bounded preparation from the same order; negative
observations are not durable blacklists. A successful switch persists the selected
backup and remaining suffix in its new encrypted snapshot, so restart cannot return
to earlier alternatives. No request-specific cursor, job queue or retry record grows
with attempts. Concurrent operations can duplicate bounded preparation; only one
expected revision commits. A lost response after commit is resolved by GET; repeating
the old expected revision cannot switch again. Cancellation before the transaction's
commit check rolls back; cancellation after commit cannot undo a published transition.

Old media IDs remain mapped to their original source during configured grace.
Old manifests can replay existing mappings; they cannot allocate new registrations
in an inactive generation. Grace expires independently of client polling. No timeline
alignment, spoken-language identity, source freshness after exit changes or seamless
player continuation is inferred from successful preparation.

Prepared HLS serialization reserves the maximum supported grant length in every
resource URL; output admission therefore includes authenticated playback overhead.
This can reject large manifests conservatively. Operators must size the existing
playlist/grant limits together; recovery never publishes first and retries a
different source behind a media URL after an output-limit failure.
