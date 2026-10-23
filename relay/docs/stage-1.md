# Stage 1: Rust foundation and wire boundary

Historical stage-1 evidence. For current networking/authorization behavior and
changed startup/stream failures, see [stage 2](stage-2.md).

Implemented on macOS arm64 with Rust/Cargo 1.99.0. This is a runnable control
foundation, not a playable relay. MSRV is deliberately 1.99 (the installed and
verified toolchain); older compilers are not claimed. Unix file APIs support
macOS/Linux compilation; Linux runtime and production networking remain unverified.
Cargo.lock pins all resolved dependencies and enabled features are in Cargo.toml.

## Current behavior

- `cargo run --locked -- config/relay.example.toml` takes one TOML configuration.
  Provision a real mode-0600 control bearer separately and replace its file path
  in a local ignored config. This command does not provision secrets or VPN state.
- Both listeners must be distinct, nonzero loopback addresses. Private service
  bearer auth (SHA-256 digest comparison using `subtle`) precedes control parsing.
  There is no request/access log; errors contain static categories only.
- Private `/health/livez` returns alive. `/health/readyz` always returns 503
  not_ready. Both require the control bearer. Stream listener requests return 503
  vpn_unavailable; no upstream connector, DNS resolution or fetch exists yet.
- `POST /internal/v1/sessions` validates the envelope, bounded snapshot, title,
  cross-IDs, source order/count, observations, known protection, supported format,
  source URL syntax and credential scope syntax. A valid import returns 503
  state_store_unavailable. It never creates an in-memory session or publishes a
  non-durable entry URL. Idempotency-Key syntax is checked but entries are not
  retained until the stage-3 transaction exists. Other control operations are
  unavailable. Source socket/destination validation belongs to stage 2.
- Encrypted HLS import preserves provider evidence; it does not prove AES-128
  identity key handling. Stage 4 inspects the actual manifest/protection method.
  Master/child/rendition URI linkage is preserved but cannot be verified until
  the guarded manifest fetch/parser exists in stages 2/4.
- Unknown provider fields and raw metadata remain private and bounded. No source
  is re-ranked or substituted. Expired selected sources are rejected; backups
  retain expiry/order for later activation eligibility checks.
- Header parser: 64 headers, 32 KiB HTTP/1 parser buffer, 5-second header deadline,
  no keep-alive or HTTP/2. Control middleware: 16 KiB header budget, configured
  body budget and 10-second total handler deadline. A finite connection set per
  listener bounds pre-auth work (control concurrency * 2; stream count).
  Excess sockets close; excess admitted control work returns 429 immediately.
- SIGINT/SIGTERM cancels listeners, drains connections to the configured deadline,
  then aborts and joins remaining tasks. No unbounded request/task queue exists.
  No session/resource metadata is retained in this stage.

Startup checks the implemented listener, numeric/cross-limit, path-prefix,
algorithm, guard-mode and control-secret invariants. It cannot attest a missing
transport/store. API verification key parsing, encryption key validation, state
writability and guard/platform attestation become mandatory with stages 2/3;
readiness cannot succeed until those checks exist. There is no bypass setting.
Configuration for those later modules is typed and bounded but does not activate
unimplemented functionality. Deployment/private container binds remain future work.

## Importer

```
cargo run --locked --bin relayctl -- import-selection \
  127.0.0.1:8091 /path/to/control-bearer /path/to/selection.json \
  household-opaque-0001 2030-01-01T01:00:00Z request-key-0001
```

Use a current bounded expiry; that timestamp illustrates syntax only. Selection
and bearer files must be regular mode-0600 files, not symlinks. `relayctl` reads
an actual providers.v1 envelope, wraps it in relay.v1 and calls the same private
session endpoint. The only permitted destination is an explicit loopback socket;
there is no DNS, proxy, redirect or provider HTTP client. It prints only HTTP
status and returns failure for the currently unavailable session store. Control
responses/credentials are not echoed. File/body/response and exchange time limits
apply. Source files are never sent directly to any provider.

## Go compatibility and regression evidence

Provider model source: `d44ae22a774cd2bc356520b585b1eff8889ce53f`, unchanged local
`internal/model/{title,source,result}.go`. The HTTP files remain scaffolds.
Run `python3 tools/regenerate-goldens.py` to copy just those model files into a
disposable Go module, run Go encoding/json offline, and regenerate synthetic
fixtures. The provider checkout is never modified. `provenance.json` records
SHA-256 of each copied source. Go 1.26+ is required; toolchain downloads and
module/network resolution are disabled by the generator.

Golden cases: movie, TV specials (season zero), HLS audio/subtitle groups,
encrypted HLS, progressive MP4, ordered backups, and no selected source.
Go's nil collections serialize as null: required reasons/backups/rejected/providers
are nullable in the corrected schemas. Omitted selected remains distinct from a
required collection. Nanosecond durations remain signed integers, not floating
seconds. Numerically equal JSON floats may serialize as `40.0` versus Go's `40`.
Unknown JSON metadata is retained without interpreting it as established facts.

Offline regressions exercise source identity/cross-ID conflicts, unsupported
formats, protection/access failures, metric bounds, duplicate-case headers,
unsafe contexts, body/snapshot/source budgets, private secrets, listener
separation, immediate request admission, connection limits, header timeout,
real relayctl HTTP exchange, joined shutdown with incomplete headers, and a
forced drain deadline with an incomplete authenticated control body, and actual
daemon executable startup/authenticated liveness/SIGTERM exit. Twenty offline
regressions pass with the required fmt/clippy/test commands.
No offline test resolves a hostname or contacts a provider/media source.

Primary crate APIs consulted when creating the manifest:
[Axum](https://docs.rs/axum/0.8.9/axum/),
[Tokio JoinSet](https://docs.rs/tokio/1.53.3/tokio/task/struct.JoinSet.html),
[Hyper-util](https://docs.rs/hyper-util/0.1.21/hyper_util/).
The controlled Hyper/rustls upstream client is intentionally deferred to stage 2;
no default upstream client is installed in the foundation.

## Greptile review fixes

The initial review identified three valid issues, all addressed in this stage:

- Cross-ID consistency now tracks supplied IMDb/MAL IDs across the entire ordered
  source set, including conflicts between backups when the selected title omits
  an ID. Missing values remain missing; successful validation does not fill them.
- Supervisor selection prefers cancellation when both cancellation and successful
  listener completion are ready. Drained listener errors are still reported.
  Deterministic regressions cover both simultaneous success and error paths.
- `serde_json` enables `arbitrary_precision` to retain opaque JSON numeric values
  without conversion through floating point. Go-produced raw metadata now includes
  an integer beyond u64 and a high-precision decimal. Separate regressions exercise
  nested/flattened unknown fields and the same typed bytes round trip as relayctl.
  See [serde_json numeric support](https://docs.rs/serde_json/1.0.154/serde_json/struct.Number.html).

All three required Cargo commands pass after the fixes (20 tests). Review comments
and local validation do not constitute playback verification or merge approval.

## Dependency freshness

The [2026-10-15 dependency audit](dependency-audit.md) checks all 128 distinct
registry packages. All 17 direct dependencies use the latest stable release;
transitive constraints are listed explicitly. SHA-2 and TOML were upgraded and
all required checks pass after updating the lockfile.
