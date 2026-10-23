Current household-access change: fixtures now open profiles without a client
session and retain the storage owner through restart. These updated cases have
not been run under the current no-tests instruction. Production packaging builds
are separate from runtime/manual acceptance. Earlier evidence below predates the
removal of account sign-in and does not validate the new access behavior.

# Verification and delivery evidence

Baselines inspected on 2026-10-17: catalog
`3bb11147ba733acc64ea5c9de1592b8e61a6d89c`, providers
`dac13b74b16cb06c6934a639f8908e30d9d17609`, relay
`c8fe6dcc74287cef0f575a949f0dd860c4e22a4c`. Sibling working trees were clean and
remain unchanged. The reference controller/profile/library/progress code and
catalog recommendation behavior were inspected without modification.

Default verification uses Go tests with synthetic loopback HTTP servers and
private temporary SQLite/key/configuration files. No credentials, external
provider traffic, VPN attestation or paid calls are required. Coverage includes
private-network household access and stable storage-owner/profile selection,
profile/library persistence through restart, cached artwork/metadata/exact specials,
malformed/interrupted/invalid snapshot preservation, recomputed freshness,
known-ID playback without catalog access, explicit cross-ID translation,
Ed25519 signature/auth-revision correctness, create idempotency, lost mutation
reply followed by GET/restart/retry, backup exhaustion followed by refresh,
revocation and catalog deletion outboxes, duplicate/concurrent/seek progress,
wrong encryption key and newer schema rejection, and JSON/CSRF bounds.

The separately invoked `python3 tools/production_listeners.py` builds race-enabled
Go API/provider test binaries and runs a Rust fixture in a disposable copy of the
relay source, leaving sibling checkouts unchanged. It runs the production provider
HTTP server and four-provider coordinator with synthetic adapters/probe observations,
and production relay control/stream listeners, verifier, encrypted SQLite store,
HLS processing and MP4 ranges. The existing cfg(test) resolver/dial points to a
synthetic origin; a test-owned synthetic guard freshness clock supplies the existing
epoch. No production guard/destination exception is introduced.

Both HLS (including independent audio/subtitle manifest associations) and MP4
passes exercise API-owned selection import, grants accepted by the actual relay,
provider process stop/join while streaming continues, API reopen and persistent
session recovery, actual relay store stop/join/reopen on the same encrypted
SQLite/key set with fresh HTTP listeners, explicit ordered backup activation,
uncached refresh, renewal and revocation of the original grant. Returned switches
require player reload/resume. Scoped upstream authorization reaches the synthetic
origin; relay control credentials/playback grants do not. Owned provider children,
HTTP listeners, store and network tasks are stopped/joined.

These are synthetic protocol/process tests. They do not establish native-provider
resolution, real-source byte availability, media decoding, rendered subtitles,
spoken language, player timeline restoration, device playback, deployment or VPN
routing. The prior ended live allowance is neither opened nor reset. The relay's
existing live-playback matrix remains open. Clients and production deployment are out
of scope.

Local formatting, vet, race suite, command build, explicit compiled-daemon startup/
SIGINT/restart smoke, two process-owner Python regressions, and HLS/MP4 production-
listener integration pass. The dependency audit reports zero reachable/package
vulnerabilities and one module-only advisory in code not called by this service.
The compiled-daemon smoke is invoked with `FAMILY_STREAM_API_BINARY` pointing to
an explicitly built binary and `go test -race -run '^TestCompiledDaemonLifecycle$'
./internal/api`; it uses only temporary synthetic configuration and state.

Additional regressions cover failed SQLite snapshot commits, shutdown cancellation
and handler joining, full-pool recommendation category views, qualification
fingerprints, unestablished playback telemetry rejection, service-port validation,
late library metadata enrichment, exclusive database ownership, joined background
workers, distinct meaningful-episode aggregation, no-source rejection, artwork
rewriting and lossless large private
JSON numbers. Process-owner tests prove a timed-out test descendant is terminated.

Final-head CI, review status and delivery commit evidence are recorded in the PR. The explicit
cross-language integration requires the sibling service sources and cached Go/Rust
dependencies; default GitHub CI uses only this repository and stays offline with
respect to providers/media.

Adjacent baseline checks also pass unchanged: catalog/provider `go vet`, race
suites and builds; relay formatting, locked all-target/all-feature clippy with
warnings denied and locked all-target tests. No sibling source patch was needed.

Manual Greptile requests were posted to the new private repository's PR, but no
completed Greptile review was obtained. The user subsequently confirmed their
Greptile review allowance was exhausted and explicitly authorized merging without
that review. The prior external-review prerequisite is waived for this delivery;
this is not a claim that Greptile reviewed or approved the code. Local verification
and final implementation-head CI passed before this documentation-only update.
Current PR/merge and commit evidence is recorded in GitHub PR #1.
