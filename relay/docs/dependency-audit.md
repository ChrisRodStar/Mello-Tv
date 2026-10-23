# Dependency freshness audit

Checked against the official crates.io API on 2026-10-15. Policy: latest
non-yanked **stable** releases, including every registry package in Cargo.lock
across target platforms. This checks release freshness, not vulnerabilities.

All **22 direct dependencies** are at the latest stable release. The initial
manifest had constrained `sha2` to 0.10 and `toml` to 0.9; those are upgraded to
0.11.1 and 1.1.8. Every direct requirement now names its current full release as
the minimum compatible version, with Cargo.lock fixing the complete resolution.
There are 153 distinct registry packages (156 package/version entries).

| Direct crate | Locked latest stable |
| --- | --- |
| `base64` | `0.23.1` |
| `ed25519-dalek` | `3.0.0` |
| `socket2` | `0.6.5` |
| `tokio-rustls` | `0.26.6` |
| `webpki-roots` | `1.0.9` |
| `axum` | `0.8.9` |
| `chrono` | `0.4.45` |
| `http-body-util` | `0.1.5` |
| `hyper` | `1.12.0` |
| `hyper-util` | `0.1.21` |
| `libc` | `0.2.190` |
| `serde` | `1.0.229` |
| `serde_json` | `1.0.154` |
| `sha2` | `0.11.1` |
| `subtle` | `2.6.1` |
| `tempfile` | `3.27.0` |
| `tokio` | `1.53.3` |
| `tokio-util` | `0.7.20` |
| `toml` | `1.1.8+spec-1.1.0` |
| `tower` | `0.5.3` |
| `url` | `2.5.8` |
| `zeroize` | `1.9.1` |

## Transitive constraints

A blanket claim that every transitive package is latest would be incorrect.
`cargo update --dry-run --verbose` reports no available compatible changes.
These constrained package names retain older versions because their parents do not
accept the newest release. No dependency forks, overrides, or unrelated upstream
changes were introduced to circumvent those requirements.

| Package | Locked | Latest stable | Verified parent constraint |
| --- | --- | --- | --- |
| `getrandom` | `0.2.17`, `0.4.3` | `0.4.3` | ring 0.17.14 requires `^0.2.10`; other consumers use 0.4.3. |
| `windows-sys` | `0.52.0`, `0.61.2` | `0.61.2` | ring 0.17.14 requires `^0.52` for Windows ARM64. |
| `windows-targets` | `0.52.6` | `0.53.5` | windows-sys 0.52 requires `^0.52.0`. |
| `windows_*` architecture support (7 packages) | `0.52.6` | `0.53.1` | windows-targets 0.52.6 pins `^0.52.6` architecture packages. |
| `matchit` | `0.8.4` | `0.9.2` | Axum 0.8.9 requires `=0.8.4` (used on the Mac). |
| `r-efi` | `6.0.0` | `7.1.0` | getrandom 0.4.3 requires `^6`, for the UEFI efi_rng backend. |
| `syn` | `2.0.119`, `3.0.9` | `3.0.9` | Windows implement/interface macros require `^2.0`; other macros already use syn 3.0.9. The older copy is outside the Mac build graph. |
| `wasi` | `0.11.1+wasi-snapshot-preview1` | `0.14.7+wasi-0.2.4` | mio 1.2.4 requires `^0.11.0` for WASI. |
| `windows-core` | `0.62.2` | `0.100.0` | iana-time-zone 0.1.65 requires `>=0.56, <=0.62` on Windows. |
| `windows-implement` | `0.60.2` | `0.100.0` | windows-core 0.62.2 requires `^0.60.2`. |
| `windows-interface` | `0.59.3` | `0.100.0` | windows-core 0.62.2 requires `^0.59.3`. |
| `windows-link` | `0.2.1` | `0.100.0` | chrono, windows-core/result/strings and windows-sys require the 0.2 series. |
| `windows-result` | `0.4.1` | `0.100.0` | windows-core 0.62.2 requires `^0.4.1`. |
| `windows-strings` | `0.5.1` | `0.100.0` | windows-core 0.62.2 requires `^0.5.1`. |

Constraints were read from the actual resolved crates using
`cargo metadata --locked --format-version 1`; Axum's exact matchit pin was also
checked in its published Cargo.toml. `cargo tree --invert matchit` confirms the
runtime dependency path. No Linux/Windows/WASI/UEFI execution is claimed.

## Repeat and validation

```
python3 tools/audit-dependencies.py
cargo update --dry-run --verbose
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
```

The audit uses the official `https://crates.io/api/v1/crates/{name}` endpoint
for each locked crate, rejects a yanked latest release, and compares every locked
version to `max_stable_version`. An optional output path saves the full JSON
report. Four bounded HTTP workers and per-request timeouts limit registry work;
failures abort the audit rather than claiming success. It does not contact media
providers, alter VPN settings, install packages, or modify Cargo.lock.

After upgrading, fmt, strict Clippy and all stage-2 offline tests pass on macOS arm64
with Rust/Cargo 1.99.0. No source/playback/networking acceptance is implied.

Primary release/API references:
[crates.io API](https://crates.io/api),
[SHA-2 0.11.1](https://docs.rs/sha2/0.11.1/sha2/),
[TOML 1.1.8](https://docs.rs/toml/1.1.8+spec-1.1.0/toml/),
[Cargo update](https://doc.rust-lang.org/cargo/commands/cargo-update.html).

## Stage-2 additions

Before addition, the official crates.io API confirmed socket2 0.6.5,
tokio-rustls 0.26.6, webpki-roots 1.0.9, ed25519-dalek 3.0.0 and base64 0.23.1
as current stable. The full audit then checked all 153 registry package names:
all 22 direct dependencies are current stable and non-yanked. The existing
transitive constraints remain; ring adds the constraints listed above. rustls
uses only the explicitly selected ring/TLS 1.2 feature set through tokio-rustls,
without default aws-lc, compression or early data. The locked rustls release is
0.23.46. Cargo.lock is committed. No dependency overrides were introduced.

## Stage-3 additions (2026-10-15)

Official crates.io `max_stable_version` and non-yanked release records were
verified before addition: aes-gcm **0.11.1**, rusqlite **0.40.2**, getrandom
**0.4.3**, hmac **0.13.0**, fs2 **0.4.3**. The full registry audit checked **183**
package names and **27** direct dependencies. All directly requested versions
are current stable; the audit's `all_locked_latest` metric reports 26/27 because
getrandom is also present transitively as ring's constrained 0.2.17.

New older transitive constraints, confirmed against `cargo metadata --locked`:
rusqlite 0.40.2 requires sqlite-wasm-rs `^0.5.1` (locked 0.5.5; newest 0.6.1),
which requires rsqlite-vfs `^0.1.0` (locked 0.1.1; newest 0.2.0), which requires
hashbrown `^0.16.1` (newest 0.17.1). These are WASM-specific dependencies, not
executed on this Mac. Existing stage-2 transitive constraints still apply.

rusqlite explicitly enables bundled SQLite and limits; AES-GCM enables zeroize.
No dependency forks/overrides or SQLite extensions are introduced. Cargo.lock
records the complete resolved graph. APIs checked against official published
crate documentation/source: [AES-GCM](https://docs.rs/aes-gcm/0.11.1/aes_gcm/),
[rusqlite](https://docs.rs/rusqlite/0.40.2/rusqlite/),
[getrandom](https://docs.rs/getrandom/0.4.3/getrandom/),
[HMAC](https://docs.rs/hmac/0.13.0/hmac/),
[fs2](https://docs.rs/fs2/0.4.3/fs2/).

## Stage 4 (2026-10-15)

No new crates or features were needed. The official crates.io API was queried
again for all 27 direct dependencies: all remain current stable and non-yanked.
TOML's published version is `1.1.8+spec-1.1.0` (the manifest constraint is 1.1.8).
The existing committed Cargo.lock is retained unchanged; all checks use --locked.

## Stage 5 (2026-10-15)

`httpdate` **1.0.3** is now a direct dependency for all three RFC HTTP-date input
formats and strict date validation. It was already present in Cargo.lock through
Hyper; no package version or checksum changed. The committed lock adds only the
root dependency edge. Stable version/API verified against the crate's published
[latest official documentation](https://docs.rs/httpdate/latest/httpdate/).
The crates.io JSON endpoint returned HTTP 403 in this environment; no registry
API audit is claimed for this stage. Required builds/tests use `--locked`.
