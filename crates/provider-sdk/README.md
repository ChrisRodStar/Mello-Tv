# Stream TV provider SDK

The portable package interface is `stream-tv:provider@1.0.0`, defined in
`wit/provider.wit`. The host exports one bounded HTTP capability and no WASI
filesystem, sockets, environment, clock or process APIs. Providers export
`resolve(lookup-json) -> result<adapter-result-json, error-string>`.

`decode_lookup` and `encode_result` validate the existing `providers.v1` wire
contract. The trusted ingress context key remains outside guest JSON. Pure
cryptographic provider code can depend on `stream-provider-codecs` separately
from the native host transport.

The safe helpers accept at most 32 KiB of lookup JSON and serialize results
through a writer capped at 1 MiB. Result limits apply during serialization,
before allocating an oversized intermediate JSON string.

## Rust guest export boundary

The current `wit-bindgen` 0.63.0 guest `export!` macro generates unsafe export
functions/blocks and an unsafe `export_name` attribute at the invoking crate's
span. A minimal guest compiled on Rust 1.99.1 for `wasm32-unknown-unknown` fails
the required `forbid(unsafe_code)` policy even though its authored Rust has no
unsafe code. Host-side Wasmtime bindings compile under the same policy. The
host, SDK, core and provider implementation crates keep unsafe code forbidden.

The isolated compile-fail reproduction is in
`tests/fixtures/guest-export-policy`. It intentionally is not a production guest
or a workspace member. Reproduce with:

```sh
cargo build --manifest-path crates/provider-sdk/tests/fixtures/guest-export-policy/Cargo.toml --target wasm32-unknown-unknown --release
```

The runnable pattern is isolated in
`../provider-runtime/tests/fixtures/guest`: a tiny ABI wrapper denies unsafe
code by default, allowing it only in the module containing generated
`wit-bindgen` bindings/exports. Its separate implementation crate forbids unsafe
code. No handwritten pointer ABI or unsafe block is introduced. This exception
exists because canonical ABI glue handles guest pointers; provider protocol
logic must remain in the safe implementation crate.

That Rust guest is compiled for `wasm32-unknown-unknown`, wrapped as a component
without WASI, and executed by the runtime regression suite. See its README for
the reproducible build commands.
