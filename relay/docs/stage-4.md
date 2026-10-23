# Stage 4: complete supported HLS delivery

Merged in [PR #4](https://github.com/ChrisRodStar/family-stream-relay/pull/4)
as `c3faeca20f804c570a7f48a3980131d39eb03051` on 2026-10-15.
The merged tree matches reviewed head `e56334c2685dddae056fdf3b3848f25e72da1ca8`;
Greptile check 116569700922 completed with 31 files reviewed and zero new comments.
All six valid findings have fixes, regressions and resolved threads.

Implemented locally on the stage-3 baseline `7ccbb3c` (stage-3 merge `c934536`).
This stage supplies production routes for the explicitly supported RFC 8216 VOD
subset below. Overall readiness remains **false**. Progressive MP4, If-Range,
automated failover and later acceptance/operations work remain unavailable.
No upstream provider/media traffic, system VPN changes, remote work or deployment
was performed. Synthetic HTTP sockets are reachable only through the existing
`cfg(test)` transport injection; production guard/destination checks remain active.

## Implemented boundaries

- `/entry` and `/r/{rid}`, GET and HEAD, use the existing EdDSA/durable ownership
  check before delivery. The atomic lookup now supplies the sensitive registration
  and its generation with the reader lease; no second lookup of a mutable root.
- Every upstream request uses the one guarded Hyper/rustls transport. Range is a
  relay-controlled field, identity encoding is requested on every redirect, and
  incoming headers/query credentials are never copied to upstream requests.
- Manifests are bounded before UTF-8/attribute/structure parsing. Selected children
  match exactly once; the playable master retains all members and attributes of
  its associated audio/video/subtitle/caption groups. Missing groups, duplicate
  rendition names/defaults, malformed lists and unsupported fetch syntax fail.
- Resource references resolve against the final redirect URL. One durable store
  transaction precedes any rewritten output; URLs contain only opaque session/
  resource IDs and the incoming grant. Repeated loads and restart reuse IDs.
  Ancillary comments and EXTINF titles are removed; retained metadata has a narrow
  grammar and is checked against source query/credential values, including short tokens and cookie components.
  Descriptive attributes are checked once against a deduplicated set of at most
  4096 values; structural protocol numbers and relay URLs cannot collide.
- Encrypted resource metadata now includes optional manifest ancestry/selection/
  subtitle context and encryption state. Default omitted fields preserve existing
  stage-3 resource identities; old roots recover selection from their encrypted
  generation snapshot without rewriting IDs. New identity includes this context.
- Ancestor original/final URLs detect cycles including redirect aliases. Nesting
  is checked before publishing a child and again when loading it. No recursive
  server-side prefetch/crawler runs. Depth bounds, per-manifest registration and
  aggregate serialized-state budgets, existing session/global resource quotas,
  store queue and database quotas bound the graph across repeated loads.
- Explicit/implicit segment and MAP ranges retain original absolute offsets.
  Implicit offsets require a preceding ranged reference of the same URI (segment
  and MAP state tracked separately). Missing Range on constrained GET is 400;
  widened extents are 416. Closed/open/suffix single ranges use checked arithmetic;
  constrained open/suffix ranges first require metadata-only HEAD discovery, then
  send a closed absolute range and verify the unchanged total. No discovery GET
  may read outside a registered extent. Sources without usable HEAD metadata fail
  explicitly for this constrained open/suffix case. If-Range is unavailable.
- Media is pull streamed. Only a bounded prefix and a finite number of capped transport frames
  are retained for classification/replay, with no whole segment/init/subtitle
  collection or media disk writes. TS packet/header and BMFF box-prefix evidence
  overrides misleading MIME; HTML/JSON/playlists/container mismatch fail. Encrypted
  media and nonzero ranges use parsed encryption/container context, because their
  bytes need not have a file header. This is not decoder/content/language proof.
- WebVTT requires a valid header; full responses validate UTF-8 incrementally over
  frames. Arbitrary byte slices may split characters and are passed intact after
  separate bounded header evidence; they are not treated as complete UTF-8 text. Keys are collected within policy and must be exactly 16 octets. AES-128
  ciphertext/IV/key transitions pass intact; the relay does not decrypt media.
- Responses use a small header allowlist: validated status/length/range, inferred
  MIME, Accept-Ranges for media, private/no-store and no-referrer. No upstream
  cookies, URLs, validators, digests or framing are copied. Rewritten manifests
  have their own lengths. Media validates 206 extents/total/length and counts body
  bytes, confirming EOF before releasing the last advertised bytes.
- HEAD manifests perform the same bounded rewrite/commit. Media HEAD uses the
  same bounded GET prefix evidence and closes the upstream body; constrained HEAD
  without Range uses original representation metadata without consuming its body.
  Every HEAD sends no downstream body and receives the same authorization policy.
- Upstream deadlines/read idle, global/session/authority admission and frame caps
  remain transport-owned. Drop closes the upstream request. Grant deadlines use
  subsecond-accurate remaining time without JWT leeway. The owned HTTP connection
  also watches the session/generation cancellation and grant deadline independently
  of body polling; socket write idle closes stalled downstream connections.
- Existing stage-3 source-generation/CAS policy is retained: already registered
  old-generation media can drain during grace; loading an old manifest that needs
  registration after a source switch fails with revision_conflict. No timeline
  splicing or implicit source activation is added.

## Declared protocol matrix

Versions **1 through 7** are accepted only for the supported subset; a higher
version or unknown EXT tag fails explicitly. Feature minimums are enforced for
fractional EXTINF (3), BYTERANGE/subtitle groups (4), KEYFORMAT (5), non-I-frame
MAP (6), SESSION-KEY (7), and explicit IV (2).

| Tags / references | Treatment |
| --- | --- |
| EXTM3U, VERSION | framing, one supported version |
| STREAM-INF + URI | exact selected-child filter or retain compatible variants in order |
| MEDIA URI / in-band groups | complete associated AUDIO/VIDEO/SUBTITLES/CLOSED-CAPTIONS groups; attributes preserved; NONE captions allowed |
| I-FRAME-STREAM-INF | explicitly excluded in both filtered/unfiltered masters; no iframe URL published |
| INDEPENDENT-SEGMENTS, START | retain validated attributes/values |
| SESSION-DATA | validated inline DATA-ID/VALUE/LANGUAGE only; external URI unsupported |
| KEY, SESSION-KEY | AES-128, identity KEYFORMAT/version 1, checked optional IV; KEY METHOD=NONE transitions |
| TARGETDURATION, MEDIA-SEQUENCE, DISCONTINUITY-SEQUENCE | required/checked structural state; no renumbering |
| EXTINF, DISCONTINUITY, PROGRAM-DATE-TIME | preserve order/duration/discontinuity/timestamps; remove EXTINF free titles |
| PLAYLIST-TYPE, ENDLIST | VOD only; ENDLIST required; no live/event reload promises |
| BYTERANGE, MAP/BYTERANGE | checked exact absolute ranges; fMP4 init/fragment context; encrypted MAP requires explicit IV |
| Media URI | MPEG-TS, fMP4 with MAP, or WebVTT from subtitle playlist context |

Explicit exclusions: LL-HLS PART/PRELOAD-HINT/RENDITION-REPORT, delta/blocking
reload, DEFINE/substitution, content steering, DATERANGE/SCTE extensions, I-FRAMES-
ONLY, unknown EXT tags or attributes, external SESSION-DATA, packed elementary
AAC/AC3/MP3, WebVTT MAP/encryption, SAMPLE-AES/nonidentity/DRM, DASH and progressive
MP4. Arbitrary provider track metadata is not promoted into invented HLS groups.
Unrecognized non-EXT comments are discarded. Metadata containing URI/credential
syntax is rejected rather than passed through as an unchecked extension. Prefix
checks intentionally do not parse an entire ISO-BMFF file or decode TS samples.

## Offline evidence

Required commands (local Mac, locked dependency graph):

```
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
```

Executed locally on 2026-10-15: rustfmt passed; strict all-target/all-feature
Clippy passed; **106 offline tests passed** (59 library, 3 authorization, 15
foundation, 4 loopback, 25 session). All 65 baseline cases remain included.
Review outcomes and exact reviewed commits are recorded on the PR and delivery
report. The user approved merging on 2026-10-15; stage 5 remains separate.
Fixtures cover the Go handoff cases in `internal/probe/handoff_test.go` and
`playlist_test.go`: selected master/group retention, quoted commas, missing/
ambiguous selected children, and explicit/implicit same-resource range state.
No provider preview buffering or regex rewriting was imported.

The new Rust cases additionally cover URI closure/redaction, unsupported tags,
unsafe references, encryption/init/subtitle paths, MIME/prefix replay, numeric
bounds, cycle/redirect/depth rejection, durable failure before output, identical
rewrites after restart, range/header/body mismatches, GET/HEAD, active expiry,
revocation, client drop, guard loss and real-listener cancellation. Existing
stage-2/3 suites remain part of the gate.

Unverified: real upstream source bytes, decryption/decoder results, player startup,
audio/language/seek/subtitle behavior, sustained playback, memory/load measurement,
real tunnel-loss behavior, deployment/Serve and physical devices. Shared streaming
primitives implemented for HLS do not complete stage-5 MP4/If-Range acceptance.

## Review fixes

Greptile reviewed initial head `a0f22014f3db0b0a77591ec7e63194bd6dd4982f`
(review 5537060350) and raised four valid findings. Each has a regression:

- Short initial TS/init ranges use a separate capped evidence request through the
  same transport, then return exactly the requested range. Evidence reads stay
  within the resource capability, validate 206/length/total, and compare totals,
  available validators and overlapping prefix bytes before delivery. A complete
  single TS packet can provide framing evidence for a short underlying object.
- Ranged WebVTT retains arbitrary bytes, including UTF-8 continuation/trailing
  prefixes, after bounded header evidence. Full responses keep streaming UTF-8
  validation. Absolute constrained subtitle evidence is tested independently.
- URI publication inserts at structural attribute spans; rendition names cannot
  collide with internal placeholders. Output growth is checked before each insert.
- Key feature-version gates inspect parsed attribute names rather than URL text;
  query parameters containing IV/KEYFORMAT cannot create false feature requirements.

Additional metadata redaction regression covers short query tokens, Bearer token
components and cookie values while excluding authorized resource URLs from checks.
Additional integrity coverage distinguishes quoted caption group "NONE" from
the unquoted NONE sentinel, enforces string/enumerated attribute types and
explicit DEFAULT/AUTOSELECT consistency, and accepts both legal hexadecimal IV prefixes.
The first follow-up on `78d77273e43bd05934d5cadde8c66c56650bde1c`
identified one additional valid issue: harmless short query values could collide
with codec/language metadata. Validated numeric version parameters (`v`, `ver`,
`version`) and conventional language/locale parameters are now distinguished from
private opaque values. Other short private values match complete metadata tokens,
not digits embedded in codec identifiers. Full credential/opaque-value redaction
remains active. Unit and production-route regressions cover `?v=1&lang=en`, codec
and language attributes, numeric cookie flags, and retained private tokens.
A subsequent review of `4083977a5be5946d07bf35acba618ba857d815f4`
identified an overly strict AUTOSELECT rule. The RFC 8216 section 4.3.4.1
presence-aware check is restored: DEFAULT=YES permits omitted AUTOSELECT, accepts
explicit YES and rejects explicit NO. The serializer preserves omission rather
than inventing a default flag. These three cases are covered offline.
Follow-up review results are recorded on the PR for the final exact head.

Protocol sources: [RFC 8216](https://datatracker.ietf.org/doc/html/rfc8216),
[RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html).
