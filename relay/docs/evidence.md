# Evidence and primary references

## Local code inspected for this setup

Providers checkout: `family-stream-providers`, main commit
`d44ae22a774cd2bc356520b585b1eff8889ce53f`.

- `internal/model/title.go`: movie/TV TMDB IDs, IMDb/MAL and exact TV coordinates.
- `internal/model/source.go`: FetchContext separation, source URL versus selected
  child, raw metadata, units and protection/delivery observations.
- `internal/model/result.go`: providers.v1 Ranked/Probed envelope and backups.
- `internal/probe/handoff.go`: preserve associated EXT-X-MEDIA groups/attributes,
  reject absent/ambiguous selected children/groups.
- `internal/network/{client,destination,local_darwin}.go`: controlled sockets,
  tunnel DNS, redirects, limits and credential containment.
- `cmd/vidcore-preview/{main,mp4}.go`: current diagnostic behavior, including range
  semantics and content-kind handling. It buffers bounded media samples/chunks;
  that is not the production Rust streaming architecture.
- `contracts/openapi.yaml`: empty provider HTTP paths; do not claim a live provider
  REST integration from native engine/check command success.

Read-only behavioral reference:
`reference/family-stream-project/apps/api/src/direct-stream-relay.service.ts` and
`vidcore.controller.ts`. They show resource-ID reuse, nested URI rewriting,
resource MIME kinds, range/header propagation and disconnect cancellation. They
also have proof-of-concept limits, hardcoded provider identity and API-process
media delivery; do not copy those into this independent relay design.

The reference checkout already had unrelated untracked files before this task;
none is part of this relay repository or modified by this setup. Parent workspace
instructions and deployment responsibilities were read; private credential
material was not copied here.

Prior provider results (including local decoded/browser playback) establish the
provider engine/Go preview only. No Rust runtime, Rust decoder/browser check,
actual relay tunnel-loss validation or three-container deployment exists from
this setup. Every implementation-plan task remains unchecked.

## Primary external references

- [RFC 8216, HTTP Live Streaming](https://www.rfc-editor.org/info/rfc8216/):
  base playlist and rendition/key/init/range semantics.
- [RFC 9110, HTTP Semantics](https://www.rfc-editor.org/rfc/rfc9110.html):
  GET/HEAD, representations, Range/If-Range and partial/unsatisfiable responses.
- [Hyper connectors](https://hyper.rs/guides/1/client/connectors/):
  custom IO/HTTPS connector boundary; validate current APIs during implementation.
- [hyper-util client](https://docs.rs/hyper-util/latest/hyper_util/client/legacy/struct.Client.html):
  typed connector/pool boundary and Tokio integration, not a security policy by itself.
- [jsonwebtoken validation](https://docs.rs/jsonwebtoken/latest/src/jsonwebtoken/validation.rs.html):
  required claims and explicit validation policy; do not rely on defaults for
  issuer/audience/nbf/algorithm requirements.

External documentation changes over time. Choose compatible current versions and
commit the Rust lockfile during implementation; this setup does not invent
unverified crate pins or claim library defaults satisfy the guarded transport.
