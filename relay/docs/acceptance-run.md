# Opt-in acceptance run shutoff

This corrects the stage-7 between-command limiter incident. It does not authorize
another live run. All validation of this change uses synthetic bytes locally.
Normal daemon invocation and production transport policy are unchanged.

## Operator command

Prepare a private directory outside Git (mode 0700), the usual production config,
keys/store and a mode-0600 `clients.json`. That JSON is an array of argument arrays,
for example `[["/absolute/path/to/trusted-loopback-client", "PRIVATE_INPUT_FILE"]]`.
No shell is used. The client must wait/retry for authenticated relay readiness,
import the saved selection through relayctl, and use signed same-session loopback
stream URLs. Grants/URLs belong in private input files, never command arguments.
All clients, including orchestration scripts and their children, belong in this
manifest. Do not attach Safari or another existing user application: this owner
supports dedicated subprocess clients which remain in their original process group.
Decoder protocols/network sandboxing and source approval are still operator duties;
the owner does not make an arbitrary executable safe to run.

After separately approving the actual sources and run limits, use:

```sh
python3 tools/acceptance_run.py --opt-in \
  --new --state /PRIVATE/RUN/budget.sqlite --bytes 536870912 --seconds 1800 \
  --relay /ABSOLUTE/REPO/target/debug/family-stream-relay \
  --config /PRIVATE/RUN/relay.toml --clients /PRIVATE/RUN/clients.json \
  --relay-restarts 1
```

These are example limits, not renewed approval for the ended stage-7 run. The
supervisor starts the actual daemon with `CONFIG --acceptance-budget LEDGER`.
There is no alternate relay, resolver, source client or destination exception.
The same guard, bound sockets, scoped context, TLS verification, authentication,
range validation and pull-based media delivery remain in use. All daemon upstream
reads (including DoH, TLS handshake/framing, HTTP headers, redirects, nested HLS,
MP4 ranges and repeat requests) pass through the gate. Provider diagnostics and
other executables' independent traffic are outside this relay budget; never run
them under an assumption that this gate bounds their network traffic.

`--new` exclusively creates the ledger and refuses existing files. Omit `--new`,
`--bytes` and `--seconds` to use an existing still-active ledger after an interrupted
owner; first ensure its previous processes are stopped. Never recreate, copy back,
edit or delete a run ledger to restart an allowance. Every normal owner exit
marks the ledger stopped; completed/stopped/exhausted/expired runs cannot resume.
Within an active owner, an early relay exit may be restarted up to the explicitly
selected count (0 by default, maximum 4), using exactly the same ledger/deadline.
Client scripts must implement bounded retry/reload themselves; active TCP streams
are not preserved. Startup failure is an error, not playback evidence.

## Byte accounting and read-ahead

The cap is a conservative **reservation of upstream TCP receive allowance plus kernel read-ahead headroom**, before TLS/HTTP parsing. It is not the process-local media
body metric, decoded bytes, downstream bytes, or an ISP/interface packet counter.
Every connection attempt first durably charges 32 KiB for kernel read-ahead,
including failed attempts/retries. This credit is never refunded: sequential
connections cannot accumulate uncharged discarded buffers. Before each socket
read, the gate also durably subtracts up to 16 KiB, never more than
the read buffer or available allowance. SQLite IMMEDIATE transactions serialize
all connections/processes, and FULL synchronous commits happen before socket reads.
A pending read retains its credit; successful partial reads use that credit before
reserving again. Credits are never refunded on EOF, retry, cancellation or crash.
Unspent reservations are deliberately lost on restart. A final reservation that
exhausts the ledger is charged but not read, immediately cancelling the gate.
Consequently received application bytes never exceed reservations or the cap;
a small allowance may stop before delivering useful media. The report explicitly
labels `reserved_receive_bytes`; it must not be presented as exact downloaded media.

TLS and Hyper may read ahead **within already charged bytes**; there is no extra
application read allowance. At most 16 KiB credit is held per socket. The existing
transport limits bound socket/driver concurrency. In acceptance mode each real
production socket also requests a 16 KiB kernel receive buffer **before connect**
and fails closed if the reported buffer exceeds 32 KiB. Kernel TCP receive queues,
packets in flight, retransmission/link/IP/TCP/VPN overhead and traffic in another
process are outside application accounting. For a network/download approval,
the ledger already reserves 32 KiB of kernel payload for every connection attempt,
not merely each concurrent connection. Allow separate headroom for in-flight and
transport overhead; this is not an exact wire-byte cap. No claim about
remote-server bytes sent, OS packet counters or VPN packet loss follows from it.
Synthetic connector tests intentionally bypass actual VPN/socket setup.

The private ledger is small metadata, not a media spool or the session database.
It must live on a reliable local disk in a trusted mode-0700 directory, with the
ledger mode 0600. SQLite access failure/lock timeout (100 ms) fails closed. Opt-in
reservation commits are synchronous and can add latency; this is acceptance
safety tooling, not a production throughput benchmark. Arbitrary OS/disk hangs
and scheduler starvation are not hard real-time guarantees. Wall-clock changes
forward end a run early; monotonic deadlines prevent a running daemon/owner from
extending its deadline on rollback. A daemon restart under the same owner still
has the original absolute UTC deadline and the owner's monotonic bound. Restarting
the entire owner during a system-clock rollback is outside that guarantee.

## Shutoff and process ownership

The persisted absolute UTC deadline is set once when the run is created, before
process launch. Each daemon also installs a monotonic timer. The socket wrapper
checks deadline/cancellation before each receive/write, including stalled reads;
the network owner watches the same token/timer, invalidates the guard and aborts
and joins its connection tasks. Infrastructure exit triggers the existing daemon
listener/store shutdown. This does not rely on a client issuing another request.

The Python owner has its own monotonic deadline and checks terminal ledger state
every 20 ms. That polling only initiates **process cleanup**, never byte admission.
At a limit, completion, signal or exception it seals the ledger, signals all owned
process groups, escalates to SIGKILL after 150 ms and waits for every direct child
(up to 3 seconds each). It observes exit without reaping leaders (kqueue on macOS, waitid/WNOWAIT on
Linux), keeping their process IDs reserved until group cleanup. Replaced relay
leaders are joined and removed from the ownership set before a restart; stale
process-group IDs are never retained for a later shutdown. This also catches
ordinary child processes. Raw stdout/stderr is discarded, never buffered or printed. Output is
fixed categories and numeric counts; decoding/player fields stay false.
Cleanup can finish after the transfer deadline. Escaped/daemonized children,
external browsers and SIGKILL/host failure of the owner cannot be guaranteed joined;
use dedicated clients that do not detach. The daemon still independently enforces
its own receive gate/deadline if the Python owner fails. Reports claim only direct
children joined, not waitpid ownership of grandchildren reparented by the OS.

## Offline validation

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools -p 'test_*.py'
cargo test --locked --lib acceptance_probe_uses_production_listener_for_hls_and_mp4 -- --ignored --nocapture
```

The opt-in integration runs the existing graph/range probe through real runtime
listeners, verifier, encrypted store and Delivery with this gate enabled. It
checks positive budget debits and closure for nested HLS audio/subtitle/video and
MP4 HEAD/three ranges; only the existing cfg(test) resolver/dial boundary targets
synthetic loopback bytes. A separate production Delivery regression exhausts
256 KiB with concurrent MP4 range clients/retries and checks guard cancellation.
Gate tests receive exactly 98,304 bytes from a fast 256 KiB producer under a
100,000-byte allowance (the last 1,696 bytes are charged and left unread), aggregate
eight independent ledger connections, preserve used credit/deadline on reopen,
and wake stalled readers on both exhaustion and time. Ownership tests cover wall
expiry, byte stop, restart debit preservation, SIGTERM resistance, ordinary child
cleanup, partial launch failure, redaction and refusing reset/symlink/expired state.
These are protocol/safety measurements, not decoding, live VPN, player/device,
Server, Serve or deployment acceptance. All 148 baseline Rust tests remain.

Executed validation for this correction: fmt and locked all-feature clippy passed;
**156 Rust tests passed** (all 148 baseline tests plus eight new regressions), one
cross-language test ignored by default. The explicitly invoked production-listener
synthetic HLS/MP4 test passed. **18 Python tests passed** (nine baseline plus nine
owner regressions). Cargo.lock is unchanged. No live media/provider requests,
VPN changes, devices or remote/deployment actions were performed.

Greptile corrections: reproduced and fixed a panic from repolling the completed
shutoff future; repeated read/write calls after both byte cancellation and deadline
now keep returning safe errors. Corrected the descendant test to let its leader
exit, verify that exit before cleanup, and retain a second stalled client until
the deadline before checking that the orphaned child was stopped.


## Coordinated HTTP-service run additions

New ledgers also contain `acceptance_requests`, an independent persistent count of
upstream HTTP exchanges. `--new --requests N` sets it once (default 4096, max
100000). Every production transport exchange, including DoH and redirect hops,
consumes one credit before connection setup; failed attempts are not refunded.
The counter survives daemon/netcheck restart and is shared through SQLite. The
owner stops/joins clients when it reaches zero and reports http_exchange_limit
and http_exchanges_admitted. A legacy byte-only ledger remains compatible but
makes no request-count claim; it is not suitable for the new live plan. The
receive-boundary byte reservation behavior above is unchanged.

`relay-netcheck CONFIG [PRIVATE_SELECTION [selected-variant]] --acceptance-budget
LEDGER` now uses that same production gate, so pre-run attestation also consumes
the original allowance. It never creates or replenishes the ledger.

Optional `--restart-file PRIVATE_MAILBOX` (mode 0600, parent 0700) requires at
least one --relay-restarts permit. Initialize it to {"request":0}. A test-owned
client atomically requests {"request":1}; the owner signals only its unreaped relay
child with SIGTERM, observes exit, cleans/joins its old group, and restarts with
the same ledger/config. It acknowledges {"completed":1}; clients then retry the
persisted session. No PID is exported and no allowance is reset. Drain can consume
the remaining wall/byte budget; restart/client recovery can be incomplete. The
mailbox supports one intentional restart, not a general process-control endpoint.

See [current HTTP integration](http-integration-acceptance.md) and the
[unapproved live proposal](http-integration-live-plan.md). Independent Go traffic
is never covered by the Rust ledger.

The synthetic HTTP-service fixture invokes service_acceptance.py with
--owned-test-client: a ten-second client owner reuses launch/exited/stop_all,
retains its direct child unreaped, and stops its process group before joining it.
The Rust twenty-second outer timeout first sends SIGTERM to this owner and waits
for cleanup before hard escalation. A regression stalls a client with a
SIGTERM-resistant provider-like descendant and verifies both are stopped. Normal
live work still uses acceptance_run.py as the whole-run ledger/process owner.

Numbered private restart mailbox requests allow up to the already configured
restart count (maximum four). The initial mailbox is `{request:0}`. Clients
request `{request:N}` in strict order; after drain/exit/restart the owner writes
`{completed:N}`. The ledger/deadline never changes. Existing single-restart
clients retain N=1 behavior; out-of-order/repeated requests fail closed.

## Owned native playback extension

The optional `--provider-budget-file PRIVATE_GO_LEDGER` watches the independent
Go allowance while clients run, stops the whole run when it ends, and seals it
after owned group cleanup. It neither creates nor replenishes Go credits. The
provider service keeps accounting/reserving its own traffic; owner polling is
only a shutdown trigger, not Go or Rust byte admission. A malformed/partial Go
record fails closed. The original Go ledger format/locks remain unchanged.

The concrete decoder/native-player extension is in
[the fresh live plan](http-integration-live-plan.md). `playback_operator.py prepare`
is offline and records binaries/tool hashes; `run --opt-in` is single-use and
requires fresh human approval. The native/FFmpeg clients inherit the manifest
client's group; they are never launched in detached sessions or attached to an
existing application. Native subtitle glyph checks use a custom visible overlay
and transient in-memory rasterization; no capture/text is written. The generic
owner still reports decoded/player fields false: detailed evidence belongs to the
owned playback client's independently validated redacted report.

Explicit offline native fixture check on macOS (the input is generated synthetic
media, not source media):

```
xcrun swiftc tools/playback_player.swift -o /PRIVATE/owned-player \
  -framework AppKit -framework AVKit -framework AVFoundation
PYTHONDONTWRITEBYTECODE=1 python3 tools/playback_synthetic.py \
  --ffmpeg /ABSOLUTE/EXISTING/ffmpeg --player /PRIVATE/owned-player
```

Prepare with `python3 tools/playback_operator.py prepare --player /PRIVATE/owned-player
--ffmpeg /ABSOLUTE/EXISTING/ffmpeg --openssl /ABSOLUTE/OPENSSL3`. The returned private
directory contains no live allowances until a separately approved `run --opt-in`.
Build production daemon/netcheck binaries in the documented local paths first;
preparation performs no builds or package installation itself.
