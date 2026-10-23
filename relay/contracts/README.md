# Contract artifacts

Files describe the complete relay.v1 target; stage-1 Rust types and the validating
private create endpoint implement its foundation. Durable sessions remain unavailable. OpenAPI references sibling JSON Schema 2020-12 files; resolve
these locally rather than fetching their `.invalid` documentation identifiers.

`providers.schema.json` follows actual providers.v1 Go JSON, including the nested
Ranked -> Probed -> Source/Observation shape and nanosecond durations. It is a
wire compatibility snapshot, not the provider service's unimplemented HTTP API.

`relay.schema.json` specifies private session requests and redacted responses.
The create request takes an existing providers.v1 selection, not a guessed flat
URL. It requires a selected item and caps backups at 127 as a relay admission
policy; larger provider results are valid provider output but cannot silently
bypass relay limits. Runtime validation must enforce title equality, protection,
format, URL/socket policy, source expiry, lease caps and credential scopes too.
Schema format annotations alone are not a security boundary.

Examples are synthetic and use a fixed 2030 fixture clock, not live source data.
Do not paste those timestamps/grants into a real session and call it working.
No grant/signing/control/encryption secret is included. The HLS fixture files are
text-only manifests; there are no playable media or encryption key files.

The current header allowance follows provider FetchContext. New fields/tag support
require versioned compatibility decisions, documentation and meaningful fixtures;
never patch provider-specific URL/token handling into the relay transport.
