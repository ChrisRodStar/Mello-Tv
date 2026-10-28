# Profile: personal library overview

Profile now opens into one scrollable overview: an identity header, Continue
Watching, My List, and recent History. The larger profile name and circular Switch
control share one row. Viewing preferences open from a gear button on the avatar. Each section
has a native navigation link to its complete collection. The overview limits its
previews to six ongoing titles, ten saved titles, and three history records;
the full collections retain every record supplied by the existing store.

The finished detail and player screens informed the charcoal palette, semantic
system typography, bare play glyphs, red progress bars, eight-point artwork
corners, and restrained surfaces. The page uses artwork and spacing for hierarchy
rather than the previous centered account header and segmented library picker.

Artwork starts or resumes playback through the existing resume helper. My List adopts the pure 2:3 compact poster card format matching Search, eliminating redundant under-card title and year labels. History items are housed in native charcoal card containers (`#1c1c1e`) bordered with subtle 0.5-point borders (`white 8%`), displaying a 16:9 thumbnail preview with red progress line and play glyph alongside clean, single-line title and episode metadata. Text and chevrons open title details, where the existing bookmark controls manage My List.
Completed TV records keep their next-episode behavior; completed movies keep the
existing playback behavior. History identifies individual title/season/episode
records, rather than treating every episode of a series as the same row.

Profile switching, viewing preferences, refresh, details, player dismissal and
refresh after playback retain their existing wiring. There are no API, persistence,
app identity, orientation-policy, or shared player/detail changes. Resume lookup
has a pending indicator, prevents duplicate requests, and cancels when this screen
leaves. A failed lookup retains the existing fallback to title details.

Text wraps using Dynamic Type; history switches to a vertical layout at
accessibility sizes. Collection grids adapt to available width and all pages
scroll vertically in portrait and landscape. Controls use native buttons/links,
minimum 44-point targets, descriptive VoiceOver labels, and textual progress.
Initial loading and failure/retry are distinct from empty collections.

## Validation and manual acceptance

Device build and installation results belong in the delivery report. They do not
establish visual or playback acceptance. No automated tests or playback probes
are part of this work.

On the iPhone Air, review Profile in portrait and landscape; open and return from
each complete collection, title details and viewing preferences. Check a long
profile/title name, large accessibility text, VoiceOver, loading/empty/error states,
and profile switching. Confirm existing saved titles and history remain visible.
Playback and resume behavior remain for user acceptance.
