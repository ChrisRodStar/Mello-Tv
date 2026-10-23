# Streaming bodies, MIME and HTTP ranges

## Bounded incremental delivery

Only control JSON, HLS manifests, encryption keys and bounded content-prefix
inspection can be fully collected. Segment, initialization, subtitle media and
MP4 responses stream through bounded pull-based bodies. Do not buffer a whole
segment or video, use an unbounded producer channel, or write media to disk.

Bound concurrent streams globally, per session and per authority. Acquire slots
with a finite queue/wait deadline before opening upstream work. Release slots,
connections, body readers and tasks on client disconnect, cancellation, revoke,
VPN loss, expiry and shutdown. Honor downstream backpressure; a slow client must
not keep an eager upstream reader filling memory. Limit transport frames/chunks
in addition to application buffers and avoid hidden library read-ahead.

Separate DNS/connect/TLS/header deadlines, upstream read-idle timeout and
client-write-idle timeout. Progressive MP4 must not inherit the provider client's
eight-second whole-body deadline or the preview's fixed short request deadline.
Its absolute bound is the smaller of transfer policy, grant and session expiry.
A long active stream is not a hung idle stream. No retry after response bytes
have been committed; abort with a safe metric rather than append an error body.

## Resource classification

| Resource | Response treatment |
| --- | --- |
| Valid rewritten HLS | application/vnd.apple.mpegurl |
| MPEG-TS segment | video/mp2t only after TS-compatible framing/evidence |
| fMP4 init/fragment | video/mp4 or appropriate validated audio media type |
| AES-128 identity key | application/octet-stream; bounded key representation |
| Proven WebVTT | text/vtt; validate header/UTF-8, do not relabel SRT |
| Progressive MP4 | video/mp4 with correct GET/HEAD/range behavior |

Upstream extensions/MIME can lie: prior sources returned real TS under text/html,
while VidFast delivered fMP4. Reject HTML/JSON error bodies and container mismatch;
do not turn every unknown response into video/mp2t. Bounded sniffing may establish
TS packet sync/header bounds or ISO-BMFF boxes, then replay the inspected prefix
into the stream without byte loss. These checks are container evidence, not decoded
video/audio or language identification. Nonzero ranges can begin inside arbitrary
compressed payloads: do not demand an MP4 file header or TS sync at every range
start. Use validated initialization/container evidence and resource context for
such ranges; validate body extents independently. Never read an unbounded prefix
just to classify a seek response.

Forward only an explicit response-header allowlist. Preserve applicable
Content-Range, Accept-Ranges, accurate Content-Length, representation MIME and
unchanged-resource validators. Strip hop-by-hop, Set-Cookie, upstream redirects,
source-identifying diagnostics and unsafe cache headers. Rewritten playlists get
new lengths/no-store; byte-for-byte media preserves representation length/encoding.
Disable transparent content decompression and request identity encoding for range
fidelity. Never mix encoded/decoded byte positions.

## GET, HEAD, Range and If-Range

Support one RFC byte range: `start-end`, `start-`, `-suffix`, with checked uint64
arithmetic and known representation-length handling. Reject malformed, duplicate
or multipart ranges before work (400); valid unsatisfiable ranges return 416 and
`Content-Range: bytes */length` where known. Do not confuse these cases. HEAD
matches GET headers/status semantics with no response body and no full media read.

Relay controls Range and optional If-Range. Validate 206 Content-Range bounds,
returned extent and total before committing headers. Verify exact body count while
streaming; truncate/overlong or changing representations terminate the response.
Advertise the requested extent, not a smaller internal fetch chunk. AVFoundation
rejects a 206 whose advertised length does not match its requested range.

For MP4, unconditionally requested ranges that are ignored by upstream must fail
explicitly instead of pretending the full 200 body is a 206 slice or draining an
unbounded prefix. If-Range validation failure can legitimately produce a full 200;
preserve that representation behavior and never synthesize stale validators.
If range discovery is needed, use a small bounded `bytes=0-1` GET and validate
its headers; do not require HEAD support from every upstream.

An implementation may issue bounded internal range windows, but must retain the
client-visible exact extent, strong consistency of total/validator and position,
backpressure and cancellation. Never clamp a large open-ended client range to
8 MiB and advertise that as its complete answer. Default direct streaming of a
valid upstream 206 avoids assembling a complete MP4 in memory.

HLS byte ranges are resolved from parsed playlist state. Authorize the resource's
registered range; a client cannot widen it to fetch unrelated bytes. Cover explicit
and implicit ranges, init ranges, repeated URI references, suffix arithmetic and
seek requests across multiple generations without byte substitution. Keep the
original absolute offsets and representation total, as specified in HLS delivery;
never rebase a constrained object to zero. Reject a constrained media GET without
Range (400), or any extent outside its registration (416). If-Range fallback to
a full 200 is forbidden for constrained objects, because it would exceed the
capability; fail safely instead. HEAD carries no body and can describe the original
representation.

Primary semantics: [RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html).

## Stage-5 implementation policy

Production entry/resource routes now implement this contract through the existing
guarded transport. MP4 ranges use a separate bounded first-box evidence GET
(`bytes=0-15`), validate total, available validators and overlapping bytes, then
stream the exact requested absolute extent. This works without upstream HEAD.
Full MP4 GET/HEAD use bounded prefix inspection of GET and close unused HEAD
bodies. These checks establish ISO-BMFF framing, not codec/decoder acceptance.

Constrained HLS open/suffix discovery first tries metadata-only HEAD. If metadata
is unavailable, a one-byte GET at the registered capability's first absolute
offset discovers the total without reading outside it. The later response must
agree with discovered total and available validators. No unconstrained GET or
full-response If-Range fallback is allowed for such capabilities.

If-Range is parsed before upstream work and forwarded as a relay-controlled
conditional on every validated redirect. Duplicate or malformed conditionals
with Range are 400; If-Range without Range is ignored. 206 requires a confirmed
strong ETag match or exact Last-Modified date match with the RFC 9110 intermediary
date-strength evidence (Date at least one second later). Weak ETags never match;
obsolete HTTP-date formats are accepted. A mismatching unconstrained conditional
may return a freshly inspected 200; matching-but-ignored and unconditional
ignored ranges fail. Conditional evidence uses a second bounded transport slot;
exhausted admission fails explicitly rather than exceeding configured limits.

Unchanged media preserves syntactically valid ETag, Date and Last-Modified only;
duplicate validators are rejected. Hop-nominated fields are stripped. ETags that
contain source URLs/hosts or imported credential/private query values are omitted.
Rewritten manifests retain their own length and no source validators. Changed
representation metadata in trailers aborts the body. Extent/length/EOF checks
continue throughout streaming, with the final advertised chunk held until EOF.
See [stage-5 evidence](stage-5.md) for measured and unverified boundaries.

BMFF evidence recognizes both normal and extended-size headers within the same
16-byte MP4 prefix. Conditional 416 responses need not repeat validators; when
missing, a bounded metadata probe independently confirms the condition and total.
Conditional 206 may omit Last-Modified: bounded comparison evidence can supply
that check internally, but no omitted upstream validator is invented in output.


Stage 7 player compatibility: syntactically valid Range requests for rewritten
manifests and bounded keys are ignored, returning their complete validated 200
representation on GET/HEAD. No upstream Range/If-Range or partial Content-Range is
used for these resource kinds. Malformed ranges still fail explicitly. This
permits players that open HTTP inputs with bytes=0- without relaxing registered
segment/init/MP4 range constraints.
