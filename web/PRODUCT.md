# Family Stream

<!-- impeccable:product-schema 1 -->

## Platform

Web app, designed for phones and television-sized screens.

## Users

Household members browsing movies and television. Profiles share one private household installation.

## Product Purpose

A simple, private catalog for finding and watching movies and TV, with personal lists, recommendations, and viewing progress for each profile. Household accounts authenticate access; the API enforces profile ownership.

## Operating Context

The app is intended to run on a Raspberry Pi 5 or another always-on computer behind Tailscale. Local development uses a Vite frontend and API; production serves the built frontend from the API. SQLite holds profile and viewing data.

## Capabilities and Constraints

- Browse Home, Movies, and TV with TMDB catalog data, profile-personalized shelves, cached catalog search with asynchronous discovery, title details, seasons, and episodes.
- Create and manage up to six profiles. New profiles select genres and favorites through the public API; favorite selection can be skipped when the catalog is still warming.
- Profiles have Continue Watching, My List, and Watch History. Playback progress is measured per movie or episode and saved to SQLite for resume.
- Recommendations combine onboarding choices and meaningful recent viewing. Snapshots persist across API restarts; ranking uses weighted genre, favorite and measured-viewing signals without an AI dependency.
- Playback uses a custom fullscreen player with native HLS or hls.js, keyboard controls, and a mobile landscape layout. The Go API orchestrates native Go providers and a separate Rust relay; clients receive authorized same-origin stream URLs.
- Household setup, password login, HttpOnly sessions and logout are implemented. Production remains private behind Tailscale.

## Brand Commitments

- Keep the product a focused Netflix-style browsing app, not a promotional landing page. Do not show logos in navigation, filler slogans, coming-soon notices, roadmap text, or placeholder actions.
- Use a side rail on larger screens and bottom navigation on phones. Make controls large and keyboard focus clear, including for television-sized displays.
- Search has its own view, without a redundant heading or decorative search icon. Show real results and useful loading, error, and empty states; omit unavailable metadata.
- Keep title details centered and compact, with artwork fading into the content background. Season selectors show season names only.
- Align episode artwork and title with the right-side info control; place runtime beneath that control. Show descriptions only after the viewer requests them.
- Continue Watching starts playback at the saved position. A separate Info action opens title details. Completed episodes can continue into the next available season when explicitly requested.

## Product Principles

- Prioritize browsing and useful actions over decoration.
- Preserve real content, clear hierarchy, and responsive layouts across phones and large displays.
- Keep profile-specific data isolated by profile ID while recognizing that profiles are not security boundaries.
- Make playback, recommendation, loading, failure, and empty states understandable without promising provider availability.
