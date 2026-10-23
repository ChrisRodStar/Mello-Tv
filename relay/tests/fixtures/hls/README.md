# Synthetic manifest expectations

These are parser/rewriter fixtures only. They contain no playable media, source
credentials or actual keys. Inject resource bodies in offline tests.

- master.m3u8: selecting video/1080 retains exactly that STREAM-INF plus complete
  audio-en/subs-en groups and attributes, including the comma in English, SDH.
  No raw upstream URI survives rewriting. CLOSED-CAPTIONS=NONE is valid.
- video-fmp4.m3u8: init and media fragments remain distinct kinds; preserve the
  discontinuity, media sequence, durations and end marker.
- audio.m3u8/subtitles.m3u8: independent companion playlists are authorized and
  rewritten too, not dropped when the selected video child is served.
- aes128.m3u8: two separately registered keys, exact IVs, METHOD=NONE transition.
- byterange.m3u8: init 0..1023, first media 1024..5119, second media 5120..9215.
  Preserve original absolute offsets and require ranges within each registration;
  never silently rebase offsets after giving a resource a new ID.
- unsupported-llhls.m3u8: return a typed unsupported manifest error; do not return
  a partially rewritten playlist containing external or unregistered part URIs.

Add negative fixtures for missing/ambiguous groups, byte-range ambiguity/overflow,
relative/final redirected paths, loops/depth, data/file URIs and malformed UTF-8.
