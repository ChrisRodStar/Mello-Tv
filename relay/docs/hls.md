# HLS parsing, filtering and rewriting

Target ordinary HLS VOD with MPEG-TS or fragmented MP4, clear or AES-128 identity
keys, plus associated audio/subtitles/captions. Parse attribute lists with quoted
commas/escapes and preserve meaningful original attributes; regex-only global URI
replacement is not the production parser.

## Master/rendition integrity

Fetch `source.url`, bounded to 1 MiB by default, and resolve against its final
redirect URL. Require valid UTF-8 and EXTM3U framing with a supported playlist
version. Reject malformed/ambiguous structures before returning a partial body.

When selected_variant_url exists, match its resolved URI exactly once in the
master. Retain that STREAM-INF and its original AUDIO/SUBTITLES/VIDEO/
CLOSED-CAPTIONS references. Retain complete associated EXT-X-MEDIA groups,
including DEFAULT, AUTOSELECT, FORCED, LANGUAGE, CHARACTERISTICS, CHANNELS,
INSTREAM-ID and URI where present. CLOSED-CAPTIONS=NONE is not a missing group.
Missing or ambiguous required groups fail explicitly. Preserve global/session
metadata relevant to the retained variant. Do not invent language/default flags.

If no selected child is present, preserve valid compatible master variants in
order rather than adding an independent ranking policy. A media playlist is not
a master. Preserve separate audio and subtitle playlists; never promote only the
video child to the client root. Any retained I-frame playlist must reference an
explicitly allowed compatible representation or be explicitly excluded under the
filter policy, not accidentally left as an unrestricted upstream URL.

## Resource registry and supported tags

| Reference | Relay resource kind and handling |
| --- | --- |
| Master variant URI lines | playlist; selected/allowed video playlist |
| EXT-X-MEDIA URI | audio/subtitle/video playlist as declared; absent URI can be valid |
| EXT-X-I-FRAME-STREAM-INF URI | iframe playlist if supported by filter policy |
| Media segment URI lines | segment_ts or fragment_mp4 from parsed stream context |
| EXT-X-MAP URI | init_mp4, including its BYTERANGE |
| EXT-X-KEY / EXT-X-SESSION-KEY URI | key; AES-128 identity only, preserving IV/key transitions |
| External proven WebVTT track URL | subtitle; validate WebVTT, not raw SRT relabeling |
| EXT-X-BYTERANGE / MAP BYTERANGE | maintain exact offsets/ranges on the referenced resource |

Resolve relative, absolute and scheme-relative URIs against the final document
URL; preserve query values as opaque upstream data. Validate URL policy before
registering and again at connection establishment. Output only same-origin
`/stream/{sid}/r/{rid}?st={grant}` references. Upstream URLs/context must not leak
through tags, root responses or errors. Sanitize/reject ancillary metadata fields
that embed sensitive upstream URIs rather than blindly publishing raw text.

Stable IDs bind session, generation, normalized upstream URL, resource kind,
context fingerprint and range constraint. Repeated manifest loads reuse IDs and
do not exhaust state. Transactionally persist new registrations before returning
the rewritten manifest. Cap manifests, nesting depth, registration count and
aggregate state. A signed session must not become an unbounded recursive crawler.

Retain media sequence, discontinuities, timestamps, durations, target duration,
ENDLIST and ordering. No sequence renumbering or segment timeline substitution.
Track implicit byte-range offsets only for the required preceding reference to
the same resource; reject ambiguous offsets/overflow. A range-scoped resource ID
must not grant access outside its registered extent. Preserve original absolute
byte offsets in the rewritten playlist: resource IDs represent the original
upstream representation, not a rebased slice. Constrained media GETs require a
single Range contained entirely in that registered extent; missing Range returns
400 and a wider range returns 416. Forward the exact absolute range and preserve
its validated Content-Range total. Do not subtract the registered offset. HEAD
may return original representation metadata without granting access to its body.

## Supported/unsupported extensions

Do not silently pass unrewritten URI-bearing extensions. LL-HLS PART, PRELOAD-HINT,
RENDITION-REPORT, delta updates, blocking reload, variable substitution, content
steering and other unimplemented fetch-bearing extensions return a safe unsupported
manifest error until deliberately implemented with full closure. Ordinary unknown
non-fetch metadata can be retained only if it cannot leak source credentials or
alter unsupported security/timeline semantics. Document supported tag/version
capabilities; fail closed on parser uncertainty.

SAMPLE-AES, non-identity KEYFORMAT, DRM license flows and DASH are unsupported in
relay.v1. Pass AES-128 encrypted media and keys intact to the authorized player;
the relay does not decrypt media or rewrite cipher IVs. Key URIs receive the same
auth/SSRF/expiry policy as segments. METHOD=NONE transitions are preserved.

HEAD playlists may require the same bounded fetch/filter/rewrite to determine
accurate representation headers; send no body. Never forward upstream ETag,
Content-Length or digest headers for a changed manifest. Prefer private/no-store;
if adding validators later, derive them from the actual rewritten representation,
including authorization context, without leaking grants.

Primary protocol reference: [RFC 8216](https://www.rfc-editor.org/info/rfc8216/).
The existing Go `probe.SelectMasterVariant` and its fixtures are behavior to
match, including missing-group and ambiguous-child rejection; do not import the
Go preview's regex rewriting as the complete parser.

## Implemented stage-4 subset

The exact supported tag/version matrix, exclusions, bounds and offline evidence
are in [stage 4](stage-4.md). Unsupported features fail explicitly. No live/player
acceptance is implied.


Stage 7 adds validated legacy PROGRAM-ID (versions 1–5), ALLOW-CACHE YES/NO
(versions 1–6) and VIDEO-RANGE SDR/HLG/PQ metadata. Rewriting preserves values;
unknown extensions remain rejected. ALLOW-CACHE never changes private/no-store.
See [stage-7 live compatibility fixes](stage-7.md).

STREAM-INF also preserves the observed `YT-EXT-ABSOLUTE-LOUDNESS` extension
as unquoted finite decimal metadata. This does not authorize other unknown
attributes or URI-bearing extensions. Numeric validation rejects quoted values,
URLs, non-finite values and exponent notation before any playlist is published.

## Verified adaptive ladders

The private root resource may retain an optional measured adaptive rendition
list. Its empty/default serialization is omitted, preserving existing encrypted
resource identity and older persisted sessions. The list derives only from the
selected source and verified backups sharing the exact master URL, provider,
fetch context, codec families and audio/subtitle groups. Every rung requires
known dimensions/bitrate, three samples, protection evidence and measured
headroom; expired or higher-than-selected rungs are excluded. The root expiry
is clipped to the included source leases.

Fresh master parsing requires exact allowed URLs and their observed resolution,
BANDWIDTH, CODECS and groups. Duplicate, missing or changed rungs reject the
master. Associated rendition attributes and groups are retained; no unrelated
provider segment enters this generation. AVPlayer can adapt within that original
provider-authored ladder. Sources with only one usable rendition and progressive
MP4 remain valid fallbacks, but cannot provide invented alternative qualities.

SessionReady exposes optional available_heights from the immutable registered
root, rather than the later expanded backup snapshot. The public API can pass
those numeric capabilities to clients without exposing variant URLs. Renewal
and source activation report the currently registered generation; absent legacy
metadata remains unknown rather than invented adaptive support.

Children of a selected or verified adaptive master are registered as media-only.
If a child later changes into another master, reject it rather than exposing
additional unprobed variants. General unfiltered nested graphs retain their
existing depth and cycle limits. The optional persisted flag is omitted when
false, preserving legacy resource identity.
