# Household management

Mello TV remains a trusted-household-network app. There is no sign-in, sign-up,
mandatory pairing, or new browser authentication. The Mac Network page can show
an address QR code; scanning it opens that server address in a browser. A QR code
does not grant or restrict access. Web player source is unchanged.

## Watching and server reliability

The iPhone and iPad player reports its actual AVPlayer playing, paused, or buffering state,
position, title, profile, and device family to `PUT /api/v1/playback-activity/{session_uuid}`.
It reports state changes and otherwise sends a heartbeat every ten seconds;
`DELETE` removes the session on player teardown. The server validates the
profile, bounds the list at 128 sessions, and expires entries after 35 seconds.
`GET /api/v1/playback-activity` supplies the Mac's Watching now section.
These are client reports, not decoded-playback verification. iOS suspension can
interrupt reporting. Web clients do not report this activity; the existing total
Playback metric still includes relay activity from all clients.

The Mac retries unexpected server exits at 1, 2, and 4 seconds, then stops and
shows the error. A process that ran for five minutes resets the failure budget.
An explicit stop cancels queued restarts. The child still follows the Mac app's
stdin lifetime pipe, including Xcode Stop Running. The optional Keep Mac Awake
setting prevents idle system sleep while the server is running. It does not
promise availability through forced sleep or shutdown.

Settings > Server offers a diagnostics report containing selected app, system,
process, catalog, and media-engine status. It excludes logs, credentials,
addresses, and profile names; raw logs remain a separate explicit action.

## Profile playback and recommendations

`GET` and `PUT /api/v1/profiles/{id}/playback-settings` store audio language,
subtitle language/default, automatic next episode, and default quality separately
from recommendation taste preferences. Existing profile payloads are unchanged.
Mac Users and the iOS profile page can edit these settings. iOS applies them in
the player; requested languages require matching available tracks and quality is
a ceiling. Failure to load settings leaves automatic next episode off for that
session rather than silently overriding a saved off preference.

Users can edit tastes from the Mac, regenerate recommendations, and see the
last completed generation outcome after server restarts. iOS exposes existing
recommendation feedback and review controls, plus available shelf explanations.
“Don't use for my taste” removes that title from favorite/viewing inputs and
requests a rebuild. Existing recommendations remain available during rebuilds.
Concurrent-edit checks are best effort, not atomic conditional writes; taste and
favorite changes spanning several requests can partially succeed.

## Encrypted household backup

Settings > Backups creates a portable `.mellobackup` containing individually
consistent SQLite snapshots of accounts and catalog, the matching account
encryption key, and optionally the personal TMDB credential. Snapshotting uses
SQLite's backup API rather than copying changing live database files. The two
databases are not captured in one cross-database transaction. Combined database
and key content is limited to 256 MiB.

Backups use AES-GCM authenticated encryption with a fresh salt and a
PBKDF2-HMAC-SHA256 passphrase-derived key (600,000 rounds). Keep the chosen
passphrase to restore on another Mac. Daily automatic backups are opt-in, store
the passphrase in this Mac's Keychain, check hourly while the Mac app is running,
and retain seven files recorded as created by the app. Unrelated files are not
rotated. Backup files and temporary/rollback storage have private permissions.

Restore decrypts, checks integrity/key compatibility, and invokes the bundled
server's `--validate-data --db-path <private-staging-folder>` before stopping the
running server. That mode uses the production storage validation and opens no
network listener or provider work. Restore requires confirmed child exit and an
exclusive `.household.lock`; current servers hold the matching shared lock for
their lifetime. Close older server versions and external SQLite tools before
restoring because they do not honor this advisory lock.

A synchronized transaction marker and retained originals allow startup recovery
if the app stops during replacement. The Mac completes recovery before starting
the server; standalone servers refuse to start while a marker remains. Normal
restore failures roll back, and incomplete recovery leaves the server stopped.
A successful restore retains private previous files for recovery. An optional
TMDB credential is restored only after database commit; Keychain failure is
reported separately. App-signing keys, VPN configuration, and unrelated projects
are not included.

Compiler builds do not establish backup/restore recovery, live playback,
provider availability, or visual acceptance. Those remain manual acceptance
checks.
