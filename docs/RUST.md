# Rust development

Use the pinned toolchain in `rust-toolchain.toml`. Shared dependencies and lint
rules live in the root `Cargo.toml`; each workspace crate inherits them.

## Style and design

Follow rustfmt and the applicable guidance in
[Google's Rust style guide](https://google.github.io/styleguide/rustguide.html).
Mello TV is an application built with Cargo and Tokio: Chromium/Fuchsia-specific
build layouts, threading rules, and async restrictions do not apply.

- Keep modules focused and name them after their purpose. Add traits, generics,
  macros, and new crates when there is a concrete need.
- Prefer borrowing to deep clones; accept `&str`, slices, and references to the
  underlying value when ownership is unnecessary. Shared async state can use
  `Arc`; cloning an `Arc` is expected.
- Use pattern matching instead of check-then-unwrap. Report recoverable network,
  input, and filesystem failures with contextual errors. Preserve API responses
  and persisted data formats when refactoring.
- Prefer explicit imports and clear names. Use named fields or enums when tuple
  positions or boolean arguments would obscure meaning.
- Choose straightforward loops over complicated iterator chains. Use derives
  and standard traits when they express the intended behavior.
- Document public contracts and important error conditions. Comments should
  explain constraints and decisions rather than restate the code.

Project lint rules forbid unsafe code, unchecked unwrap/expect calls, explicit
panics, debug-print macros, and unfinished `todo!`/`unimplemented!` placeholders.
Keep any justified exceptions narrow. Tests may use assertions and scoped
unwrap/expect allowances. Standard Clippy checks stay enabled; do not enable
entire pedantic/restriction groups merely to increase the number of rules.

## Checks

Test observable behavior through production code. Avoid copying implementation
logic into tests, checking values a test just constructed, and duplicating the
same case without protecting a different boundary. Fixture setup failures must
fail the test; use deterministic local inputs rather than host configuration.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo install cargo-audit --version 0.22.2 --locked
cargo audit --file Cargo.lock
```

The macOS CI workflows enforce formatting, compilation, Clippy, and tests.
Dependency auditing checks every tracked Cargo lockfile, including the standalone
provider test fixture, on dependency changes, daily, or manually so newly reported
advisories are checked even without a code change. Fix vulnerable
dependencies rather than suppressing findings; any necessary exception needs a
specific advisory ID and documented applicability rationale.
