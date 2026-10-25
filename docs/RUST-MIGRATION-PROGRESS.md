# Rust migration progress

Updated 2026-10-25. Initial implementation milestone: shared contracts and storage.

## Implemented

- A Rust 2024 workspace pinned to Rust 1.99.1, using resolver 3 and inherited unsafe/unwrap/expect/panic lint rules. Cargo.lock records the resolved dependency graph. Unused dependencies from the draft were removed.
- The existing relay is explicitly excluded until its transport/platform migration. Its independent Cargo metadata now resolves; its behavior and deployment are unchanged.
- Native macOS/Linux/Windows CI for formatting, warning-free checks, Clippy and Rust tests, using pinned action commits and locked dependencies. The workflow has not been dispatched by this work.
- `stream-core` contracts for catalog metadata/snapshots/recommendation inputs, public profiles/playback/progress, provider discovery/probes/selections, and independently versioned provider package manifests. Existing JSON names, positive signed-64-bit IDs, TV specials, scoped credentials, nullable Go fields and nanosecond durations are preserved. Validation and diagnostic redaction guard untrusted values.
- Provider package manifests describe host/interface compatibility, artifact digest/size, bounded resource requests and exact origin capabilities. These are metadata contracts; signature verification and module execution are later stages.
- `stream-storage` owns a bounded, ordered SQLite actor on a dedicated blocking thread. Accepted mutations finish after caller cancellation. Ordered, repeatable shutdown drains work and closes file handles. Filesystem permission adapters are isolated from portable domain logic; Windows creation inherits parent ACLs and requires installer/service hardening at packaging time.
- Catalog schema v0/v1 to v2 migration, existing schema ownership checks, atomic batched writes/revisions/quotas, stale reads, literal search, detailed-title preservation, cross-ID overlay, and coherent raw exports. Existing artwork/jobs are preserved. Raw exports are private storage data and do not implement public recommendation enrichment.
- API schema v1 compatibility and full definition validation, preserved AES-GCM nonce-prefix envelopes/key sentinel, household owner selection, profiles/preferences/lists, idempotent playback allocation, encrypted initial staging, initial readiness registration, monotonic per-playback progress, cumulative history/activity, and profile deletion cascades.
- Typed catalog/profile repository ports retain freshness and require account ownership. Incompatible/newer/unowned databases and incorrect keys fail before schema/sentinel or journal changes.

## Verification

- Workspace formatting, `cargo check --workspace --all-targets --locked`, and `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Rust contract and storage tests cover legacy JSON/schema fixtures, independent AES-GCM ciphertext fixtures, future-schema/key rejection, transaction rollback, profile isolation, idempotency, seeks/out-of-order progress, cache budgets/expiry/version isolation, concurrent access, caller cancellation and shutdown/reopen.
- `stream-core` also cross-checks all targets for `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu` from this Mac. Cross-checking is not native execution or a complete application platform pass.
- No Go tests, live provider/TMDB requests, real database migrations, device runs or deployment actions were performed during this implementation milestone. All storage tests use temporary/in-memory databases.

The final chat report records the exact passing test count. No playback speed improvement is claimed; the new media path is not implemented yet.

## Remaining migration work

1. Provider SDK/component runtime proof, signed package verification, generation activation/draining/recovery, and compilation/instantiation measurements. Package manifests alone do not provide independent updates.
2. Portable direct/HTTP/SOCKS5 transport preserving destination validation, DNS/redirect policy, credential scoping, budgets and connection reuse. The draft HTTP client is not an approved production transport.
3. Native HLS/MP4 probing, full eligibility/ranking/coordinator behavior and five provider ports with exact protocol vectors. Preserve progressive startup and background backup completion.
4. Existing relay integration: remove VPN binding/attestation, adapt Unix signals/secret storage/locking, align shared SQLite/dependency versions, and retain its media/auth/recovery regression corpus. Its existing Windows blockers remain.
5. Catalog upstream refresh/jobs/recommendation services and public snapshot enrichment/backup, API snapshot mirroring/pending mutation recovery/revocation/background cleanup, and public exposure/authentication design.
6. The actual HTTP daemon, configuration/CLI, single-process ownership, graceful task shutdown, iOS contract integration and end-to-end acceptance. The unfinished Rust binary exits with an explicit error rather than reporting successful startup.
7. Application installers/service integration, signed release metadata and updater, restart/drain policy, schema-compatible recovery, and native platform/device/playback validation.

No production cutover, commit, push, or deployment has been performed. The existing working backend remains intact.
