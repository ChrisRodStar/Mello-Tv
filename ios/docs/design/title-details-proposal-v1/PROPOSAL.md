# Title details — proposal 01

Status: awaiting user feedback. No app implementation, build, installation, tests or playback probes have been performed for this proposal.

Reviewed on the existing `ios/title-details` branch: `TitleDetailSheet.swift`, `EpisodeRowView.swift`, `PlayerView.swift`, `FamilyStreamTheme.swift`, plus the app's playback orientation policy. The working tree was clean before adding these proposal artifacts.

## Visual proposal

- [Movie and TV portrait](portrait-proposal.png)
- [Landscape and adaptive rules](adaptive-proposal.png)

Artwork and titles are illustrative; these are measured schematics, not runtime screenshots. Cyan annotations are proposal guides only.

## Shared structure

Use `FamilyStreamTheme.background` (#141414), theme white and secondary text, and muted metadata. Reserve red (#e50914) for saved-progress bars and necessary status accents. Keep the current large-sheet presentation, with a plain close glyph in a 48 × 48 point target. Remove the decorative dismiss circle, outlines, shadows, vignette, negative title overlap, duplicate type badge and decorative episode dividers.

The shared reading order is artwork, type label, title, metadata, playback/My List actions, synopsis, genres/status, then TV episodes. The title and metadata precede actions so the action group belongs to the title it operates on. Retain the existing Play / Resume / Next Episode labels and My List state.

Base spacing in points:

| Element | Proposal |
| --- | --- |
| Horizontal safe-area gutter | 24 |
| Artwork | 16:9, 8 corner radius |
| Artwork to title group | 16 |
| Type label to title | 6 |
| Title to metadata | 8 |
| Metadata to actions | 16 |
| Action-to-action gap | 12 |
| Action height | Minimum 44; grows with text |
| Actions to synopsis | 16 |
| Synopsis to genres | 12 |
| Final summary content to Episodes | 24 |
| Episode heading to rows | 16 |
| Episode thumbnail | 128 × 72 at default size; 8 corner radius |
| Thumbnail to copy | 12 |
| Episode title to runtime | 8 |
| Runtime to synopsis control | 6 |
| Between episode rows | 20 |
| Saved-progress bar | 4 high at thumbnail bottom |
| Bottom content padding | 24 plus system safe area |

Use semantic typography: title2 bold title, headline section headings, subheadline semibold episode titles and actions, subheadline body copy, caption metadata and runtime. All text scales. Main title and metadata wrap without shrinking. The diagrams illustrate overall hierarchy rather than exact SF font metrics.

Keep My List a plain icon-and-text action, with a minimum 44-point target. A white filled primary action has 6-point corners and no shadow. Preserve separate episode play and synopsis-expansion buttons: tapping the thumbnail/title plays; the synopsis control expands text without playing. Expanded synopsis spans the row width below the preview/copy group with a 12-point gap, keeping its full text readable. No extra circular play button is necessary. Saved progress remains sourced from the existing library store; missing history does not display a fabricated zero percent.

## Adaptive behavior

Choose layouts from the actual offered width and Dynamic Type, not a device orientation check. A two-column hero fits only when artwork can occupy at least 280 points and text at least 320 points, separated by 24 points. Artwork takes approximately 40% of content width, capped at 360 points. Both columns align at the top. Synopsis stays in the text column, directly after actions; the hero grows to its content.

TV episode rows remain below the hero. Use one column normally and two only when each column can fit at least 360 points, with a 24-point gutter. Preserve reading order. Movie details end after their own content without stretching to fill the viewport.

At accessibility Dynamic Type sizes, use one content column. Stack actions and the season header when needed. If a thumbnail beside text leaves insufficient readable width, put the preview above the episode copy and allow the row to grow. Preserve full title and expanded synopsis access. One vertical scroll container supports short landscape windows and large content. The close control remains reachable above scrolling content.

The current playback orientation policy already leases landscape only while playback owns the window. Detail views use the normal app orientation policy; this proposal requires no global landscape restriction or playback-policy rewrite.

## Preserved states and boundaries

Keep existing playback requests, resume selection, saved season restoration, season selection, library mutations, progress storage, preloading and API contracts. Title loading/error/retry stays near summary metadata; episode loading/error/retry stays inside the episode section. Image placeholders preserve geometry. Keep the same actions and data flow; restyle and reflow their presentation.

Expected implementation scope: `TitleDetailSheet.swift` and `EpisodeRowView.swift`, with detail-local helper views if needed. Read the player and theme as references; avoid changing unrelated playback controls or global theme behavior.

## After design approval

Implement on `ios/title-details`, build the device app and install over the existing app on the paired iPhone Air, retaining its application identity and data. Do not uninstall. Report compilation/signing and device installation separately from user visual acceptance. Do not run automated tests or playback probes. Do not merge or push.
