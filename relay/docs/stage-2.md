# Stage 2: controlled transport and authorization

Stage 2 implements the networking and authorization boundary, not durable sessions
or playable media. Development and verification used macOS arm64, Rust/Cargo
1.99.0. Linux live egress fails closed: namespace/firewall attestation is not yet
implemented. There is no platform, proxy, private-address or TLS bypass setting.

## Production transport

`network::Transport` is the sole upstream client. Its companion `Worker` must be
run by the process owner. Startup loads pinned playback public keys and creates
this pair; the daemon supervisor drives the worker alongside both listeners and
joins shutdown. A missing worker/healthy guard blocks work. Overall readiness is
still 503 because encrypted durable state and delivery are unavailable.

An independently owned monitor polls the fixed Mullvad executable every 250 ms; each status and
interface subprocess has a one-second timeout and bounded output. It requires
connected state and an UP/RUNNING utun interface. Identity includes the endpoint,
exit location, interface name/index and first interface status line. Each identity
transition creates a new cancellation epoch; unhealthy observations cancel and
remove it. Leases older than two seconds cannot admit new work. Detection is
sampled (up to roughly 2.25 seconds including subprocess deadlines), not an
instantaneous OS event guarantee. No intentional physical tunnel-loss experiment
was performed. Binding is the additional egress boundary during observation delay.

Every IPv4/IPv6 TCP socket uses socket2's safe IP_BOUND_IF/IPV6_BOUND_IF setters
before connect. Connections dial only validated literal addresses and verify the
actual peer address. Host and rustls SNI retain the original URL authority; public
WebPKI roots verify certificates. HTTP/1 only; no ALPN/coalescing, Alt-Svc, system
resolver, environment proxies, decompression, cookie jar or automatic redirects.
No idle connections are retained (pool capacity zero). Guard loss closes active
connections even if their response bodies are not being polled. Retired epochs
cannot be reused. Driver tasks have a finite queue/set and are cancelled/joined.
Dropping a fetch or response cancels its connection; absolute deadlines also apply
to unpolled bodies. Read-idle deadlines apply when pulling body frames.

DNS uses the configured public literal bootstrap address, the same bound socket /
Hyper / verified rustls boundary and configured original DoH hostname. Cloudflare's
JSON DoH GET protocol queries both A and AAAA without a system-DNS fallback. DNS
bodies are capped at 64 KiB, DNS records at 64 addresses, question name/type checks, all answers are validated
before any source dial, and every redirect resolves again. Special-use IPv4,
IPv6 outside 2000::/3, mapped/translation/transition and documentation ranges are
denied, matching or strengthening the Go provider policy (including 192.88.99/24).
URL validation rejects encoded authorities, local names, nonstandard scheme ports,
userinfo, fragments, zones and alternate numeric IPv4 spellings.

Only GET/HEAD source requests are accepted. At most three redirects; no HTTPS
downgrade. Redirect bodies are closed without draining. Every hop rebuilds the
provider allowlisted context from normalized origin and segment-bounded paths;
cookies also require an explicit matching cookie path. Encoded/ambiguous paths
never receive scoped credentials. Conflicting matching headers/cookies fail.
There is no API accepting arbitrary client upstream headers. Client bearer,
Cookie and playback grants cannot enter the FetchContext through the stream route.
Upstream Set-Cookie is never used to acquire new context. Transport response
headers are internal; client response filtering remains a stage-5 responsibility.

Bounds include global/session/authority admission, DoH concurrency, finite driver
queue/set, 64 headers, 32 KiB Hyper parser buffer / 16 KiB accepted headers,
configured DNS/connect/TLS/header/read-idle deadlines, 64 KiB maximum returned body
frame and a caller deadline capped by the MP4 transfer maximum. Stage 3/5 callers
must clip that deadline to the authorized session/source/resource lease and pass
its revocation cancellation token. No complete media body is collected here.

## Playback boundary and stage-3 interface

Pinned keys use a mode-0600 regular non-symlink JSON file:

```json
{"keys":[{"kid":"operator-key-id","public_key":"base64url-unpadded-32-byte-Ed25519-public-key"}]}
```

At most 16 keys; duplicate IDs, weak/invalid keys and invalid files fail startup.
There are no private signing keys, JWT key URLs or network lookups. EdDSA is fixed;
strict Ed25519 signature verification, exact issuer/audience, required typed
claims, duplicate/unknown fields, canonical base64url, 4096-byte maximum,
positive auth revision, opaque IDs, timestamp ordering, bounded lifetime and
at most 30-second clock skew are enforced. Existing private constant-time service
bearer auth remains separate.

Both actual GET/HEAD entry and nested resource routes require exactly one `st`.
Wrong methods return 405; private health/control paths return 404 on the stream
listener. Valid signatures and route SIDs precede a bounded local authorization
store read. `session::AuthorizationStore` requires one consistent durable record
with owner/SID/auth revision, revocation, resource ownership/generation, source
and session expiry, old-generation retention and an active cancellation token.
`authorize` validates those fields and returns a clipped lease/token. JWT clock
skew never extends source/session/body expiry. Stage 3 must implement this atomic
read and cancellation ownership; it must not supply an in-memory substitute.
The production implementation is `Unavailable`: valid grants still return 503
state_store_unavailable. No stream response, resource URL or temporary session
is published. Creation retains stage-1 503 behavior. All stream responses use
private/no-store and no-referrer; HEAD error bodies are empty. No access logging
is enabled and errors never include URLs, tokens or provider data.

## Verification evidence (2026-10-15)

Required fmt, locked strict all-feature Clippy and locked all-target tests pass
(37 offline regressions).
Default tests are entirely offline, using cfg(test)-only DNS/socket injection and
synthetic TLS trust. Those hooks are absent from release/library builds. Existing
Go goldens, importer, private bearer, admission and joined-drain tests remain;
the daemon executable test now verifies failure on absent verification keys
before any guard subprocess rather than starting a live guard during offline tests.

Offline cases include public destination bypasses and alternate addresses, mixed
answers before dialing, re-resolution/rebinding on redirects, redirect-to-private,
three-hop limit, cross-origin/path credential and cookie isolation, proxy
poisoning in a fresh subprocess, original Host/SNI and trusted/untrusted/wrong-host
TLS, old epoch invalidation, dropped header-wait closure, production dual-query DoH
parsing with synthetic TLS, closure of an unpolled body socket, cancellation during
DNS/connect, and signed authorization attacks on real GET/HEAD entry/resource
routes plus durable-record ownership/generation/expiry attacks.

The opt-in diagnostic below runs only the production transport and a fixed pair
of Mullvad attestation endpoints. It has no HTTP listener, arbitrary URL input,
media delivery or session creation path:

```sh
cargo run --locked --bin relay-netcheck -- config/relay.example.toml
```

Both `ipv4.am.i.mullvad.net/json` and `ipv6.am.i.mullvad.net/json` returned a verified
`mullvad_exit_ip: true` through the Rust transport. Repeating with the public IPv6
DoH bootstrap `2606:4700:4700::1111` also passed both checks. This verifies actual
bound IPv4/IPv6 source sockets, bound IPv4/IPv6 DoH, TLS, parsing and pull-based
bytes for egress attestation. It does not prove physical tunnel-loss/reconnect
behavior, packet-capture absence of leaks, provider/media source accessibility,
decoded audio/video, player startup/seeking or deployment. No VPN/firewall settings,
remote hosts, provider repository or reference repository were modified.

Primary API references:
[socket2 interface binding](https://docs.rs/socket2/0.6.5/socket2/struct.Socket.html#method.bind_device_by_index_v4),
[tokio-rustls](https://docs.rs/tokio-rustls/0.26.6/tokio_rustls/),
[Ed25519 strict verification](https://docs.rs/ed25519-dalek/3.0.0/ed25519_dalek/struct.VerifyingKey.html#method.verify_strict),
[Cloudflare JSON DoH](https://developers.cloudflare.com/1.1.1.1/encryption/dns-over-https/make-api-requests/dns-json/).

Provider reference inspected read-only at `d44ae22a774cd2bc356520b585b1eff8889ce53f`:
`internal/network/{client,destination,local_darwin}.go` and
`internal/model/{source,result,title}.go`. The HTTP interface remains a scaffold;
no provider endpoint or live provider integration is claimed.

## Review follow-up

Greptile reviewed `869fe1d` and posted three findings. The valid starvation and
address-fallback findings are fixed: an independently owned monitor task cannot
block driver admission, uses skipped missed ticks and is cancelled/joined; a
per-address connect timeout advances only while the overall deadline remains,
with guard/caller cancellation still terminating immediately. Regressions cover
slow monitor progress/join and a blackholed first address followed by a working
IPv6 address, plus an exhausted total deadline.

The Linux compilation explanation does not match the resolved socket2 0.6.5
source: both index-binding methods have Linux implementations using
SO_BINDTOIFINDEX. Nevertheless, explicit compile-time Darwin isolation and a
non-Darwin fail-closed socket stub now enforce the documented platform boundary,
including cfg-gated Darwin imports/helpers. Linux compilation/runtime is not
claimed from Mac checks.
