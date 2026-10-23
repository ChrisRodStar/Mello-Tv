# Stream tv recommendations

Improve recommendations behind the existing iOS design. Do not redesign browsing
as part of recommendation work.

Keep the original multi-card showroom, numbered Top 10 row, panoramic 16:9 rows,
editorial spotlight, alternating poster rows, spacing, controls, and onboarding.
The catalog view and onboarding were restored from commit 951dcd2.

The API still supplies improved recommendation selections and row order. The app
fills the original carousel with the reserved pick and titles from the first row.
No recommendation banner, filter chips, tuning buttons, or category Hide controls.

Build and installation results do not prove visual or playback acceptance.
