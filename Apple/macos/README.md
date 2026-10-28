# Mello TV for macOS

The native SwiftUI and AppKit app manages the local Mello TV server. Onboarding finishes at the native Overview. The viewing website opens only through the explicit Open Website command.

## Source structure

- `MelloTV/App/`: application entry point, lifecycle, commands, and main window.
- `MelloTV/Navigation/`: desktop shell, collapsible sidebar, destinations, and content routing.
- `MelloTV/Core/`: configuration and server response models.
- `MelloTV/Services/`: server supervision, status polling, and login-item integration.
- `MelloTV/UI/`: shared colors and presentation components.
- `MelloTV/Features/`: Overview, Providers, Network, Settings, Onboarding, and Menu Bar.

App state and window controllers stay on the main actor. The server status store handles HTTP polling separately from view presentation. Provider activity is server-reported operational metadata. Sparkle manages updates for the complete Mac app and bundled server.

## Window sizing

`MelloTV/Core/Configuration/WindowSizing.swift` is the single source of window dimensions. The main window's opening and minimum frame are both 1150 × 750 points, including the title bar and toolbar. Its AppKit minimum and live-resize delegate use that same policy. The opening frame and minimum are applied after the first presentation realizes the toolbar, and after frame restoration, so neither toolbar chrome nor an old saved size changes the requested dimensions. Later presentations retain the user's larger window size.

Both windows embed SwiftUI inside a plain AppKit content controller with hosting sizing options disabled. Individual destinations fill the offered content area without controlling window bounds. Onboarding has separate content dimensions (excluding window chrome), explicitly named in the same configuration file. Ordinary SwiftUI component widths and heights are layout values, not window sizing rules.

## Build and install

Release builds are assembled and signed locally, using tooling outside Git tracking. The DMG contains `Mello TV.app` and an Applications shortcut for drag-and-drop installation. Signing keys and notarization credentials remain in the maintainer’s Keychain.

The bundle uses `MelloTV` for the native executable and `MelloTVServer` for its bundled server. The internal Rust package remains `stream-server`. The shared iOS Icon Composer document is compiled for macOS during packaging.

Server data and logs live under `~/Library/Application Support/Mello TV` and `~/Library/Logs/Mello TV`. Existing legacy directories are moved as complete directories on first launch, after the previous app and server have exited. If a move fails, the app continues using the existing directory. No database is reset.

## Settings

Settings stays in the native content pane, with General, Server, Updates, and About sections. General saves a Dock-only, menu-bar-only, or combined presentation preference and applies it immediately. The menu bar and application menu both expose Settings; the application shortcut is Command-comma. Launch at login is shared between General and the menu-bar command. Server contains process controls and shortcuts to logs and data. Updates shows installed app/provider versions, a manual update check, and automatic checking/installation preferences.


## Catalog setup

Open **Settings → Server → Catalog · TMDB**, then paste your own TMDB API key (v3) or API Read Access Token from [TMDB account settings](https://www.themoviedb.org/settings/api). **Save & Restart Server** validates the credential against TMDB, saves it in this Mac’s Keychain, and restarts the local server. Active streams are interrupted by the restart. The credential is never written to UserDefaults, the app bundle, logs, or Git. Remove deletes only the saved credential; profiles and cached metadata remain.

A new install starts with no catalog entries. Creating a profile starts fetching real onboarding title choices. Saving the completed profile’s genres and favorite titles starts its personalized recommendations. Missing title and season details are fetched on demand; cached metadata remains available during upstream outages. There is no bundled demo catalog. Companion iOS and web clients use the credential configured on their server, rather than storing a second credential on each viewing device.

For a server launched directly from a terminal or service manager, supply `TMDB_API_KEY` or `TMDB_READ_ACCESS_TOKEN` through its private runtime environment. Keep those values outside Git. The Mac’s saved Keychain credential takes precedence over an inherited runtime credential.

## Software updates

Mello TV uses Sparkle 2.10.1 for signed whole-app updates, including the bundled server, web player, and providers. **Check for Updates…** is available in Settings → Updates and the Mello TV menu. Background checking and automatic installation can be disabled independently in Settings. Installation waits for streaming to become idle; in-flight media delivery and recent playback/progress activity defer it. The server retains a three-minute grace period between requests, so an update may wait after viewing stops. An unavailable readiness check postpones installation. Relaunching restarts the server. Profiles, watch history, metadata, and Keychain credentials stay outside the app bundle.

Existing releases without Sparkle need one manual DMG installation to gain in-app updates. Updates must preserve compatibility with existing companion clients.

App sources and the canonical update feed live under `Apple/macos/`. Future Mac builds use `Apple/macos/appcast.xml` on this repository's `main` branch. Published alpha.3 apps still check `macos/appcast.xml`; keep that legacy URL available. At the next Mac release, publish the new signed update entry to both feeds. That release contains the new feed URL, so users switch feeds after updating. Leave the legacy feed frozen on that bridge release for people who update later; subsequent releases only need the canonical feed.

Publish a feed entry only after its signed/notarized archive is available at the final GitHub Release URL. Use increasing `CFBundleVersion` values, the product release label, and Sparkle archive signatures. Private signing keys remain in Keychain; only the public verification key belongs in `Info.plist`. An app update does not independently activate the Rust provider updater.

Local packaging links and embeds the official Sparkle framework. Use the pinned [Sparkle 2.10.1 distribution](https://github.com/sparkle-project/Sparkle/releases/tag/2.10.1); its release archive SHA-256 is `1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27`. The local packaging/signing tools handle the framework's nested helpers before signing the outer app. Do not commit framework binaries or local release tools.
