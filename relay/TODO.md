# Relay delivery stages

**Stages 1–6 are merged. Stage 7 tooling/offline evidence is prepared; live acceptance remains open.**

This is the stage overview. The [implementation plan](docs/implementation-plan.md)
contains the detailed tasks and completion gates. Update both when a stage is
completed; a checked stage means its defined scope is verified and merged.

- [x] **Stage 1 — Rust foundation and exact wire contracts**
  Runnable daemon, bounded listeners and admission, typed configuration/errors,
  Go-compatible provider models, and the private `relayctl` importer.
  Merged in [PR #1](https://github.com/ChrisRodStar/family-stream-relay/pull/1)
  as `2c87f4a`; 20 offline tests and the required Cargo checks passed.
  See [stage-1 evidence](docs/stage-1.md) and the
  [dependency audit](docs/dependency-audit.md).

- [x] **Stage 2 — Controlled transport and authorization boundaries**
  Public-destination and DNS/socket validation; Mullvad-bound Darwin DNS and
  IPv4/IPv6 sockets; guarded Hyper/rustls transport; validated redirects and
  scoped credentials; VPN epoch cancellation; playback JWT authorization.
  Gate: upstream requests cannot bypass the guard, and GET/HEAD entry/resource
  authorization fails closed without usable state.
  Implementation checklist is complete; see [stage-2 evidence](docs/stage-2.md).
  Merged in [PR #2](https://github.com/ChrisRodStar/family-stream-relay/pull/2)
  as `5d0d9e4`; 37 offline tests passed.

- [x] **Stage 3 — Durable sessions, resource identity and lifecycle**
  Relay-owned encrypted SQLite state, bounded session admission and idempotency,
  expiry/renewal/revocation, stable generation-bound resource IDs, bounded caches,
  migrations and crash restoration.
  Gate: session/resource mappings are committed before client URLs are issued
  and remain recoverable after restart.
  Implementation checklist is complete; see [stage-3 evidence](docs/stage-3.md).
  Merged in [PR #3](https://github.com/ChrisRodStar/family-stream-relay/pull/3)
  as `c934536`; 65 offline tests passed. Both Greptile findings were fixed;
  the follow-up review of `4bc5021` completed with zero new comments.

- [x] **Stage 4 — Complete HLS delivery**
  Bounded playlist parsing, master/selected-child association, complete audio and
  subtitle rendition groups, authenticated URI rewriting, segments, keys, init
  data, subtitles and byte-range state; explicit unsupported-feature errors.
  Gate: the supported resource graph is closed, registered durably, and contains
  no leaked upstream URLs or discarded rendition associations.
  Implementation checklist and offline evidence: [stage 4](docs/stage-4.md).
  Merged in [PR #4](https://github.com/ChrisRodStar/family-stream-relay/pull/4)
  as `c3faeca`; 106 offline tests and required Cargo checks passed. Six valid
  Greptile findings were fixed; final review of `e56334c` completed with zero
  new comments.

- [x] **Stage 5 — Progressive MP4 and bounded media streaming**
  Pull-based streaming, backpressure, finite buffers/slots, cancellation and
  timeouts; correct GET/HEAD, single ranges, If-Range, 206/416 and MIME handling.
  Gate: exact advertised byte extents and bounded streaming without full-media
  buffering or disk spooling.
  Implementation checklist is complete; [stage-5 evidence](docs/stage-5.md)
  records 122 offline tests and the bounded synthetic measurement.
  Merged in [PR #5](https://github.com/ChrisRodStar/family-stream-relay/pull/5)
  as `e96269b`; all three valid Greptile findings were fixed and resolved.
  Follow-up review of `77462ad` completed with zero new comments.

- [x] **Stage 6 — Failure recovery and operational integration**
  Ordered backup activation, source refresh and generation changes, explicit
  caller reload/resume, safe diagnostics, health/metrics, graceful drain,
  restored sessions and documented component activation/rollback.
  Gate: existing sessions do not require per-segment provider/API/catalog calls;
  restart and VPN exit-change effects are explicit. Container/supervisor/firewall
  packaging is explicitly deferred until a separate request.
  Merged in [PR #6](https://github.com/ChrisRodStar/family-stream-relay/pull/6)
  as `68ce3dcb8792fbb275201982458c850fc015b089`; 143 offline tests and required
  Cargo checks passed. Final review of `647512fb3e6e3a015ca15cbca7a475f350580951`
  completed with zero new comments.
  See [stage-6 implementation and offline evidence](docs/stage-6.md).

- [ ] **Stage 7 — Cross-component and playback acceptance**
  Verify the complete Rust path for movie/TV/anime HLS and progressive MP4:
  decoded audio/video, player startup/seeking/subtitles, sustained playback,
  component outages, restart recovery and explicit backup reload/resume.
  Gate: record evidence for each acceptance layer. Real tunnel-loss experiments,
  production deployment, Serve and physical-device checks require their respective
  authorization and cannot be inferred from offline tests or compilation.

## Delivery rules

Work in dependency order, with one coherent stage per PR. Implement and verify
the stage, update its checklist/evidence, commit/push, request Greptile review,
address valid findings and rerun affected checks. Wait for the user's merge
approval before starting the following stage. Silence from Greptile is not approval.

Required checks for implementation stages:

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
```

Stages 4–5 implement the documented HLS/MP4 paths and shared streaming primitives.
Overall acceptance remains false; [stage-7 evidence](docs/stage-7.md) records
prepared tooling, executed offline checks and open live/player acceptance. Runtime component readiness is reported separately. Keep compilation,
offline recovery, source accessibility, decoded media and actual player behavior
distinct in every report.

Coordinated providers HTTP implementation/offline acceptance: [current matrix](docs/http-integration-acceptance.md). Fresh live service-boundary authorization is pending; Stage 7 remains unchecked.
