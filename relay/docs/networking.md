# Controlled upstream networking

One guarded transport handles every source/media request. A valid signed session
is authorization to known resources, not an exemption from destination checks.

## Connection establishment

1. Validate URL syntax: only HTTP/HTTPS, standard ports, no userinfo, fragments,
   opaque URLs, local hostnames, IPv6 zone IDs or non-network schemes. Reject
   encoded-host ambiguity and malformed IP forms after canonical parsing.
2. Attest the connected Mullvad interface/route and capture its identity/epoch.
   Resolve DNS only over an attested, tunnel-bound resolver; no system DNS fallback.
3. Validate **every** DNS answer as allowed global unicast. Reject mixed public/
   private answers; normalize IPv4-mapped IPv6 and block special-use, loopback,
   private, link-local, CGNAT, metadata, multicast, documentation and transition
   ranges using a maintained explicit policy. Match or strengthen provider policy.
4. Connect to the validated literal IP with bound sockets and original-host SNI/
   Host, verifying TLS certificates normally. Do not perform a second hostname
   resolution inside TLS or the HTTP connector. Every happy-eyeballs/retry path
   must apply the same policy to its actual socket address.
5. Return a connection tagged with the VPN epoch and validated authority. Check
   guard/epoch before reuse; never share credentials by connection coalescing.

macOS: create IPv4/IPv6 sockets bound using IP_BOUND_IF/IPV6_BOUND_IF. Pin DNS too,
matching the Go client's fail-closed properties. A route check or `mullvad status`
alone is insufficient. Linux deployment: enforce streaming-namespace firewall and
VPN-interface-bound egress before readiness, with a guard that stops requests and
invalidates connections on tunnel/interface/exit changes. No normal ISP fallback.

A tracked guard monitor cancels active upstream bodies and flushes epoch-bound
pools when VPN health/identity changes. Merely checking before a request is not
continuous fail-closed enforcement. Guard loss makes readiness false and blocks
new work. Host/container firewall is an additional boundary, not a substitute for
local Mac socket binding or runtime evidence. Unknown/unimplemented platforms
fail startup for live egress.

## HTTP/TLS and redirects

Use one custom Hyper connector with rustls, bounded pools and explicit DNS,
connect, TLS and response-header deadlines. Disable environment HTTP proxies,
automatic redirects, content decompression, unrestricted retries, HTTP/2 origin
coalescing and alternative-service transports that evade the connector.

At most three redirects; GET/HEAD only, HTTP/HTTPS destinations revalidated on
each hop, no HTTPS downgrade, no method/body replay. Resolve against the actual
final URL when parsing playlists. Do not inherit DNS approval across hops. Bound
redirect header/body disposal and close if draining would exceed the budget.

On every hop construct headers again from the resource's FetchContext:

- Public root `Origin`/`Referer` can accompany validated CDN resources. Require
  credential-free root origins, no query/fragment/path carrying session data.
- Scoped headers/cookies require normalized scheme/host/port and path-segment
  boundary matching, including an explicit cookie path. Revalidate supplied
  scope/header values at import; do not carry them blindly after redirects.
- Keep the provider allowlist: User-Agent, Accept, Accept-Language, Referer,
  Origin, Authorization, X-Playback-Environment and X-Csrf-Token. Arbitrary header
  names, CR/LF/NUL, Host/connection framing, proxy credentials and duplicate
  conflicting controls are rejected. CSRF must stay origin/path-scoped.
- Client Authorization/Cookie/grants are never upstream headers. Range/If-Range
  are relay-owned validated controls, not arbitrary client forwarding.

Never forward upstream Set-Cookie to clients or mutate a global cookie jar from
media responses. Relay serves imported context; source/session token acquisition
belongs to providers. Native media requests and explicit context preservation may
be needed for the same media bytes; do not remove scoped playback-environment
headers as an optimization.

## Evidence and acceptance

Offline connector tests use a resolver/socket/transport double, never a production
`allow_private` switch. Include rebinding, mixed answers, mapped addresses,
redirect-to-loopback, scope boundaries, proxy environment poisoning, alternate
connections, stale pooled epoch and cancellation while DNS/connect/body waits.

Authorized live checks must attest egress from the relay process, verify both
address families/DNS, then exercise tunnel loss/reconnect under an explicitly
approved experiment. Configuration or a successful bind is not proof that no
packet escaped during failure. Do not disrupt the user's VPN during setup.

## October 19 performance follow-up

A and AAAA DoH exchanges run concurrently as caller-owned futures. Positive
answers are coalesced per hostname, bounded to128 entries, and cached for the
shortest advertised answer/CNAME TTL capped at60 seconds. Missing TTL disables
reuse; rejected or mixed private answers never become cache entries. Every
entry retains its VPN epoch cancellation lease, and address validation still
runs on hits and at actual socket establishment. A cancelled owner wakes its
waiters without leaving a detached resolver task.

HTTP/1 connections may return to a bounded idle pool only after complete body
EOF and trailer checks. The pool retains at most16 idle connections, at most4
per authority, for15 seconds; keys include authority, freshly validated peer
membership and a live VPN epoch. Scoped headers are rebuilt for each request.
Abort, incomplete body, response Connection:close, epoch loss, request timeout,
idle timeout and shutdown discard connections. Driver futures remain supervised
by the existing owned/joined worker. A stale pooled GET/HEAD gets at most one
fresh guarded attempt within its original deadline. Request reservations and
new-connection reservations retain their existing accounting boundaries.

Fixed diagnostic counters expose DNS cache hits and successful connection reuse.
Offline regressions prove dual-query overlap, caller cancellation/coalescing,
TTL/epoch invalidation, body-complete reuse and header-scope isolation. They do
not substitute for deployment, real-player, tunnel-loss or DNS leak evidence.
