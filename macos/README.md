# Mello TV for macOS

The native SwiftUI and AppKit app manages the local Mello TV server. Onboarding finishes at the native Overview. The viewing website opens only through the explicit Open Website command.

## Source structure

- `MelloTV/App/`: application entry point, lifecycle, commands, and main window.
- `MelloTV/Navigation/`: desktop shell, collapsible sidebar, destinations, and content routing.
- `MelloTV/Core/`: configuration and server response models.
- `MelloTV/Services/`: server supervision, status polling, and login-item integration.
- `MelloTV/UI/`: shared colors and presentation components.
- `MelloTV/Features/`: Overview, Providers, Network, Settings, Onboarding, and Menu Bar.

App state and window controllers stay on the main actor. The server status store handles HTTP polling separately from view presentation. Provider activity is server-reported operational metadata; update availability remains unconfigured until a release source is supplied.

## Window sizing

`MelloTV/Core/Configuration/WindowSizing.swift` is the single source of window dimensions. The main window's opening and minimum frame are both 1150 × 750 points, including the title bar and toolbar. Its AppKit minimum and live-resize delegate use that same policy. The opening frame and minimum are applied after the first presentation realizes the toolbar, and after frame restoration, so neither toolbar chrome nor an old saved size changes the requested dimensions. Later presentations retain the user's larger window size.

Both windows embed SwiftUI inside a plain AppKit content controller with hosting sizing options disabled. Individual destinations fill the offered content area without controlling window bounds. Onboarding has separate content dimensions (excluding window chrome), explicitly named in the same configuration file. Ordinary SwiftUI component widths and heights are layout values, not window sizing rules.

## Build and install

Release builds are assembled and signed locally, using tooling outside Git tracking. The DMG contains `Mello TV.app` and an Applications shortcut for drag-and-drop installation. Signing keys and notarization credentials remain in the maintainer’s Keychain.

The bundle uses `MelloTV` for the native executable and `MelloTVServer` for its bundled server. The internal Rust package remains `stream-server`. The shared iOS Icon Composer document is compiled for macOS during packaging.

Server data and logs live under `~/Library/Application Support/Mello TV` and `~/Library/Logs/Mello TV`. Existing legacy directories are moved as complete directories on first launch, after the previous app and server have exited. If a move fails, the app continues using the existing directory. No database is reset.

## Settings

Settings stays in the native content pane, with General, Server, Updates, and About sections. General saves a Dock-only, menu-bar-only, or combined presentation preference and applies it immediately. The menu bar and application menu both expose Settings; the application shortcut is Command-comma. Launch at login is shared between General and the menu-bar command. Server contains process controls and shortcuts to logs and data. Updates reports installed app/provider versions without claiming an availability check.


## Catalog setup

Open **Settings → Server → Catalog · TMDB**, then paste your own TMDB API key (v3) or API Read Access Token from [TMDB account settings](https://www.themoviedb.org/settings/api). **Save & Restart Server** validates the credential against TMDB, saves it in this Mac’s Keychain, and restarts the local server. Active streams are interrupted by the restart. The credential is never written to UserDefaults, the app bundle, logs, or Git. Remove deletes only the saved credential; profiles and cached metadata remain.

A new install starts with no catalog entries. Creating a profile starts fetching real onboarding title choices. Saving the completed profile’s genres and favorite titles starts its personalized recommendations. Missing title and season details are fetched on demand; cached metadata remains available during upstream outages. There is no bundled demo catalog. Companion iOS and web clients use the credential configured on their server, rather than storing a second credential on each viewing device.

For a server launched directly from a terminal or service manager, supply `TMDB_API_KEY` or `TMDB_READ_ACCESS_TOKEN` through its private runtime environment. Keep those values outside Git. The Mac’s saved Keychain credential takes precedence over an inherited runtime credential.
