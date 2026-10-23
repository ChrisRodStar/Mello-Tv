# Proposal 03 — episode-first details

Approved by the user with the removal of the “More episodes” text. Supersedes earlier compositions. Episodes continue naturally as the page scrolls; there is no scroll instruction label.

The rejected area was the Resume/My List action row and excessive introductory content. Replace the two-button row with one full-width playback action. Move My List into a bare bookmark/check control on the artwork with a 44 × 44 pt target, explicit accessibility label, saved state and action announcement. Keep close as a separate 48 × 48 target. Position image controls inside system safe areas.

Put the full title in a compact artwork header (~200 pt at default portrait text size, growing for wrapped titles). Use only a localized dark image scrim needed for legibility. No separate title, type label, rating, genres or synopsis before TV episodes.

Below the image: 16 pt gap, one full-width Play/Resume/Next Episode action, minimum 44 pt height with 6 pt corners. Follow with 16 pt gap to the Episodes heading and season selector, then 16 pt to the list. Resume may include known season/episode context without changing existing target selection. Preserve existing movie action labels.

TV uses compact vertical episodes: 128 × 72 thumbnails, 8 pt corners, 4 pt saved-progress bar, 12 pt image/copy gap, 16 pt screen gutters. Episode title, runtime/progress and an independent synopsis-expansion control sit beside each preview. Minimum 44 pt synopsis hit area; whole thumbnail/title region remains a separate play action. Row spacing 20–28 pt depending on text; no borders, dividers or circular play buttons. Preserve full episode title access at large text sizes.

Place the existing title synopsis, year, rating, runtime and genres in an initially collapsed About section after the complete episode list. Movie uses the same image/title/action header followed by its synopsis and quiet metadata, with no episode section. Keep loading/error/retry visible in their relevant sections even if About is collapsed.

Use semantic Dynamic Type. Grow the hero for title wrapping; stack actions/headers or image/copy when necessary. Landscape uses a shallow image crop with full title legibility and vertical scrolling. Keep normal detail rotation and existing playback-only landscape policy.

Preserve playback/resume, saved season selection, My List, progress, preloading, loading/error/retry and API contracts. No source edits, build, installation or tests before design approval. After approval build and install over existing app on paired iPhone Air, preserve app identity/data, report build/install separately from visual acceptance. No automated tests, playback probes, merge or push.
