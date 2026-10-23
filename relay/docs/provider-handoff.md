# Provider compatibility and handoff

Audited against `family-stream-providers` commit
`d44ae22a774cd2bc356520b585b1eff8889ce53f` on local/remote main. Source of truth:
`internal/model/{title,source,result}.go`, `internal/probe/handoff.go`, and the
shared selection implementation. The paired HTTP service now defines POST /internal/v1/resolve and private health routes; see [current integration](http-integration-acceptance.md). The wire Selection envelope is unchanged.

## Current providers.v1 envelope

`version` is `providers.v1`. `selected` is optional/null if no source qualified.
Each selected/backup item has `candidate: { source, observation }`, plus `score`
and `reasons`. `backups` are already ordered. `rejected` maps candidate IDs to safe
reasons, not candidates. `providers` contains source-resolution counts/status.
Do not flatten this envelope or mistake a diagnostic score for provider priority.

At session import, reject missing selection, wrong version, source/title mismatch,
unsupported observed format, inaccessible/failed observation or unknown protection.
The API's trusted import still does not exempt URLs from destination validation.
No full media download or source re-ranking occurs during import.

| Go JSON field | Relay handling |
| --- | --- |
| `candidate.source.id`, `provider`, optional `server` | Private source identity; current providers are vidcore, vidlink, vidsrc, vidfast |
| `candidate.source.title` | Exact movie/TV + positive TMDB; preserve IMDb/MAL; TV requires season >= 0 and episode >= 1 |
| `candidate.source.url` | Playable master or progressive URL; private only |
| `candidate.source.selected_variant_url` | Selected video child, never a replacement for an HLS master |
| `candidate.observation.format` | Observed container; HLS/MP4 supported in relay.v1; DASH/unknown rejected |
| `candidate.source.fetch_context` | Persist as scoped credentials plus separate public root Origin/Referer |
| `candidate.source.expires_at` | Optional RFC3339 timestamp; per-source lease, not whole-session expiry |
| `source.resolver_metadata`, `observation.playlist_metadata` | Keep bounded unknown fields privately; never turn missing fields into known metadata |
| `observation.variant` | Preserve groups/codecs/resolution and source-selection evidence |
| `observation.encrypted`, `protection_known` | Preserve facts; support clear/AES-128 HLS only, reject unsupported protections explicitly |
| `content_verified` | Preserve as reported; relay delivery never upgrades it automatically |
| `backups` | Keep source ordering; no fixed preferred provider and no timeline splicing |

Go `time.Duration` fields serialize as integer **nanoseconds** (`first_byte`,
`resolver_duration`, provider `duration`). `delivery_bps` is **bits/second**, not
bytes/second. Go pointer/omitempty values can be absent; missing bitrate, expiry,
language, codecs or dimensions must not be replaced with inferred values.

`Metadata.provider_data` contains arbitrary raw JSON and may include signed URLs
or unknown expiry/protection details. Encrypt it in durable snapshots and exclude
it from public endpoints, diagnostics and metric labels. `tracks_known: false`
is distinct from an authoritative empty track list. `englishTrackIndex` is not
language evidence. An SRT label is not proof of usable WebVTT.

## Import constraints

- Match every retained backup's type, TMDB ID, season and episode to the selected
  title. Preserve supplied cross-IDs and reject conflicting IDs when both supplied.
- Treat selected_variant_url as an exact resolved-URI match after redirects; require
  exactly one occurrence in the master and every referenced rendition group.
- Source format and observed format must agree when both known. Use observed HLS/
  MP4 if the source reports unknown; do not pretend unknown/DASH is MP4.
- Retain known source expiries; skip expired alternatives with a safety margin.
  A source lease may expire before a still-authorized relay session.
- Bound the private import body and source count; oversized snapshots return an
  explicit error, never silently truncate or reorder. The caller may deliberately
  submit an ordered prefix within limits, preserving the selected entry and order.
- Validate each header, scope origin/path boundary, cookie path and public identity.
  Never accept arbitrary client headers as a provider FetchContext.

## Cross-language fixtures

`contracts/providers.schema.json` records the actual selection nesting and public
Go JSON names. It is a compatibility snapshot, not a deployed provider API spec.
Additional diagnostic/metadata fields remain allowed within byte budgets. The
synthetic import example follows these shapes and contains no real source URLs.

Stage 1 adds Go-produced golden JSON files (`tests/fixtures/providers/`) for movie, TV specials,
HLS with separate audio/subtitles, encrypted HLS, progressive MP4 and ordered
backups. Rust regressions assert optional/null/unit semantics. Mutation regressions cover
for malformed identity, missing selected source, unsupported version, conflicting
cross-IDs and changed/ambiguous master linkage. Do not import private captures into Git.

See [stage-1 evidence](stage-1.md) for generator/provenance and the schema correction
for Go nil collections. Master linkage validation remains a stage-4 gate.
