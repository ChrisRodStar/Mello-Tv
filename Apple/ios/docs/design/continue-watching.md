# Continue Watching

Home, Movies, TV, the Profile overview, and Profile’s complete Continue Watching
collection share a compact 2:3 poster card matching My List. Horizontal cards are
126 points wide (180 with accessibility text); the complete collection uses the
same adaptive grid as My List. Artwork is the playback target; a small independent info control opens title
details. There is no title below the poster. One centered metadata caption inside
the artwork, immediately above the progress line, combines season/episode and
remaining time over a localized contrast scrim. Full episode names remain
in the playback accessibility value.

The episode-detail preview informs the bare central play glyph, subtle contrast
overlay and four-point progress line. There is no large playback label, chevron,
percentage badge or text below the poster. Text wraps with Dynamic Type.

The card distinguishes Resume from Next episode for completed TV records. The
caption identifies the recorded episode and whether it is finished. Next episode
uses the existing lookup, including its fallback to details when no next episode
can be resolved. No future episode coordinates or artwork are guessed. Remaining
time is derived only from finite recorded progress, with an In progress fallback.

Horizontal shelves retain safe content margins and aligned scroll targets.
Profile retains six overview items and View all.

A pending indicator and playback disabling avoid duplicate resume lookups. Catalog
screens own/cancel the lookup on departure or profile change. Existing saved-position,
next-episode, details and refresh-after-playback behavior remains connected to the
same helper and API. Filtering, persistence, provider resolution, player controls,
and the approved profile identity/preferences/My List/history layouts are unchanged.

## Acceptance

The physical-device build passes. Build/install results establish packaging and
installation only; no automated tests, playback probes or live-media validation
are included. The implementation is left uncommitted for review.

On iPhone Air, inspect Home, Movies, TV, Profile’s shelf and View all in portrait,
landscape and larger text. Check long titles, episode labels, remaining time,
missing artwork, swiping and details targets. Resume, completed-TV next-episode
lookup, cancellation and playback refresh remain manual behavioral acceptance.
