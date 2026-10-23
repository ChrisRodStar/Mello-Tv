# Viewing preferences

The two-step flow retains the existing format, genre and favorite-title contracts,
minimum three selections per step, generation/save sequence,
existing selections and completion callbacks. No changes to the player, details,
approved Profile page, API, or persistence are part of this redesign.

The visual direction follows the finished app: charcoal surfaces, semantic system
type, white primary actions, red selection accents, eight-point artwork corners,
and generous spacing between sections. Genre names now sit below proportioned
artwork instead of clipping inside fixed-height images. Favorite titles render one
poster and one wrapping label, replacing fixed-width PosterTile nesting and the
duplicate title. Checkmarks and VoiceOver selected state supplement selection color.

Format uses three explicit choice buttons. The heading sits beside Back and Close controls without repeating the profile
name, avatar or step label. Editing an existing completed profile includes Close
without saving.
Back from favorites preserves the draft. The footer’s favorites summary opens a separate sheet for
removing saved selections even when they are absent from the candidate choices. Title identities include media type so movie and TV IDs cannot collide.

Layout uses available container dimensions to budget artwork height. At normal
text sizes the iPhone Air composition is designed to fit the available screen:
compact format buttons, eight genres in two portrait columns or four landscape
columns. Favorites use one continuous scrolling grid of every returned title,
with adaptive columns and naturally proportioned posters. There are no paging
controls or page numbers.
Labels remain separate from artwork. The footer’s count opens the full favorites
list in a sheet instead of expanding the page. Poster images fit their bounds.

The primary action stays in the bottom safe area. Accessibility text and unusually
short windows retain scrolling, wrapping labels and an inline action rather than
clipping or shrinking controls. Footer controls stack when necessary.
Rotation keeps the draft. Physical-device fit remains part of
visual acceptance; compilation is not evidence that all layouts fit.

Loading retains a full-size action with a spinner. Failed title loading and saving
preserve the draft and expose retry through the same action. Missing genre artwork
uses a stable placeholder without preventing selection. An empty candidate response
provides Reload and Back; saved favorites remain accessible. Leaving the view cancels
its owned operation; edits are submitted only through the final save action.

## Manual acceptance

Build/install evidence is separate from visual and behavioral acceptance. No
automated tests, playback probes or live recommendation requests are agent validation.

On iPhone Air, open the avatar gear. Review genres, format and favorites in portrait
and both landscape directions, then with larger accessibility text. Confirm labels,
selection indicators and the footer remain visible and all content/actions are reachable.
Change orientation mid-selection. Open the favorites summary and remove a selection from outside the visible grid. Check Close leaves saved settings untouched; then save deliberately and verify
the choices persist. Also check fresh-profile setup and offline/retry/empty-artwork
states when convenient. Those flows remain manual acceptance.
