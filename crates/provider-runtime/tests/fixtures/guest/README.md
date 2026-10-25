# Portable Rust provider fixture

This fixture proves independent portable Rust component loading. It validates a
`providers.v1` lookup and returns an empty measured-candidate list; it does not
contact upstream sites or establish provider availability.

`implementation/` contains the safe provider code with `unsafe_code = forbid`.
The tiny wrapper denies unsafe code by default and grants a narrow exception
only to its generated `wit-bindgen` ABI module. There is no handwritten unsafe
code. The host, SDK and other workspace crates retain unsafe prohibition.

Build from the repository root using the pinned toolchain:

```sh
rustup target add wasm32-unknown-unknown
cargo build --locked --manifest-path crates/provider-runtime/tests/fixtures/guest/Cargo.toml --target wasm32-unknown-unknown --release
cargo run -p stream-provider-runtime --example package_guest -- crates/provider-runtime/tests/fixtures/guest/target/wasm32-unknown-unknown/release/stream_provider_fixture_component.wasm crates/provider-runtime/tests/fixtures/guest/component.wasm
cargo test -p stream-provider-runtime --test components real_rust_component
```

The checked-in `component.wasm` is portable WebAssembly, not native serialized
Wasmtime code. The regular test suite executes it without requiring a local
guest toolchain. Its Rust source, dependency lockfile and build command are
included for review/reproduction. Runtime module staging authenticates its
length and digest against the supplied manifest, while release signature
verification belongs to the update subsystem.
