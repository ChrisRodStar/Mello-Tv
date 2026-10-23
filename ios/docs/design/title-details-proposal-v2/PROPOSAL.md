# Proposal 02 — artwork-led continuous page

Awaiting feedback. Supersedes proposal 01. These annotated schematics are illustrative, not app screenshots. App source is unchanged.

The user rejected the right-shifted composition and unused left space. Remove the two-column hero and thumbnail-beside-copy episode rows. Use full-width artwork followed by one left-aligned content group. Remove the redundant Details/type labels. Keep a bare close glyph in a 48-point safe-area target. The artwork may extend to screen edges; controls remain within safe areas.

Content gutters: 16 points inside the safe area. Portrait artwork keeps a 16:9 composition; in short landscape windows use a shallow crop, approximately 160–200 points high (180 illustrated). Artwork-to-title gap: 16. Title-to-metadata: 8. Metadata-to-actions and actions-to-synopsis: 16 each. Synopsis-to-genres: 12. Summary-to-episodes heading: 24. Heading-to-episode artwork: 16. Primary action: at least 44 high, 6-point corners; My List remains plain with a separate minimum 44-point target.

Use a horizontal episode filmstrip that closely follows the player browser: 232–300-point cards chosen from offered width (248 in portrait illustration), 20-point card spacing, 16:9 thumbnails, 8-point clipping, 4-point saved-progress bars. Copy sits beneath each thumbnail: 12 to title, 8 to runtime, 6 to synopsis. Preserve a separate synopsis expansion action. The complete card has no enclosing background, border or shadow. A partially visible next card signals horizontal scrolling. Artwork in schematics represents placeholder illustration only; actual app uses the current catalog assets and loading/failure placeholders.

Semantic fonts scale with Dynamic Type. Main title wraps and metadata reflows. Action labels stack when needed. Episode cards can widen for readability; at accessibility sizes use a vertical episode list with preview above copy, no fixed text heights. Expanded episode synopsis is available without triggering playback. Use a vertical page scroll view; the normal-size episode shelf scrolls horizontally. In landscape, all sections still follow one continuous leading edge beneath full-width artwork. Both movie and TV use this shared header; movie ends after its content.

Keep the theme's #141414 surface, white primary type, muted metadata and #e50914 progress accent. Keep existing playback/resume and next-episode selection, saved season restoration, My List, progress, preloading, loading/error/retry behavior and API contracts. Detail views use normal app rotation; landscape policy remains playback-scoped.

After explicit approval: implement on ios/title-details, build and install over the existing app on the paired iPhone Air, preserving identity and data. Report build/install evidence separately from visual acceptance. No automated tests, playback probes, merge or push.
