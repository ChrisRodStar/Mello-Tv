# Provider component runtime

`ProviderRuntime` loads portable components implementing
`stream-tv:provider@1.0.0`. It offers no WASI capabilities. An injected
`HttpTransport` owns actual networking; it must enforce DNS/address/proxy and
redirect/credential policy on every connection and bound decompressed responses
while reading. The runtime independently checks manifest origins, methods,
headers, request counts and cumulative request/response bytes.

`stage` checks manifest/host/interface compatibility, exact artifact size and
SHA-256, permitted imports and static memory/table requirements, then compiles
off the async executor. It does **not** authenticate a signature: downloaded
packages must first pass the update layer's signed metadata verification and
capability approval. Native serialized code is never accepted/deserialized.
`resolve_staged` lets the update layer exercise fixture lookups before activation;
failures leave the current generation intact. A successful compile or fixture
does not establish current upstream availability.

Compilation, staged packages, retained compiled-code weight, lookup admission
and aggregate guest memory all have bounds. Each store allows one memory, two
tables, 4096 table elements and eight instances. Manifest memory, HTTP and
deadline limits apply per lookup. An engine epoch thread lets infinite CPU
loops yield even under a single-thread async executor. Lookup cancellation and
deadline expiry drop host HTTP futures; injected transports must not detach I/O.

Activation atomically switches one provider generation. Existing handles retain
their old code/manifest and continue independently. Upgrades must exceed the
highest activated semantic version (build metadata cannot bypass the check).
Explicit local recovery selects the retained previous version without lowering
the upgrade watermark. That watermark is currently in-memory; persistent
package-version policy remains the update/application layer's responsibility,
alongside TUF metadata rollback/freeze protection.

The compiled cache is bounded using generated-code serialization size as a
retained-code weight; this is not an exact process RSS limit. Compiler transient
allocations and lifted result strings are additionally bounded by source size,
compilation admission and guest memory, not by the JSON output length alone.
No latency/RSS performance improvement is claimed by the fixture tests.

`tests/components.rs` executes real component-model fixtures, including the
portable Rust component described in `tests/fixtures/guest/README.md`.
