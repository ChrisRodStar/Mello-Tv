# Mello TV for macOS

The native SwiftUI and AppKit app manages the local Mello TV server. Onboarding finishes at the native Overview. The viewing website opens only through the explicit Open Website command.

## Source structure

- `MelloTV/App/`: application entry point, lifecycle, commands, and main window.
- `MelloTV/Navigation/`: desktop shell, collapsible sidebar, destinations, and content routing.
- `MelloTV/Core/`: configuration and server response models.
- `MelloTV/Services/`: server supervision, status polling, and login-item integration.
- `MelloTV/UI/`: shared colors and presentation components.
- `MelloTV/Features/`: Overview, Providers, Network, Updates, Onboarding, and Menu Bar.

App state and window controllers stay on the main actor. The server status store handles HTTP polling separately from view presentation. Provider activity is server-reported operational metadata; update availability remains unconfigured until a release source is supplied.

## Window sizing

The main window defaults to 1150 × 750 points, with a content minimum of 1150 × 750 points. AppKit owns these bounds; the hosting controller does not derive new window limits from individual destinations. Content must adapt to the proposed pane width. Section selection must not resize the window.

## Build and install

Run `./macos/package-macos.sh` from the repository root. It builds the web assets and Rust server, compiles the native `MelloTV` executable, and creates `dist/macos/Mello TV.app` and a DMG.

The bundle uses `MelloTV` for the native executable and `MelloTVServer` for its bundled server. The internal Rust package remains `stream-server`. The shared iOS Icon Composer document is compiled for macOS during packaging.

Server data and logs live under `~/Library/Application Support/Mello TV` and `~/Library/Logs/Mello TV`. Existing legacy directories are moved as complete directories on first launch, after the previous app and server have exited. If a move fails, the app continues using the existing directory. No database is reset.
