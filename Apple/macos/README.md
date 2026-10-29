# Mello TV for macOS 🖥️

The native Mac app for **Mello TV**. It lives in your menu bar and Dock, manages the local streaming server, and lets you manage providers, check network connections, and control server settings without touching a terminal.

---

## What It Does

- **One-Click Server**: Starts and stops the bundled Rust server quietly in the background.
- **Menu Bar & Dock**: Keep it tucked away in your menu bar, run it in the Dock, or both.
- **Native Dashboard**: Check your providers, see who's streaming, view system logs, and change settings in a clean SwiftUI interface.
- **Automatic Updates**: Built-in Sparkle updater checks for updates and installs them safely when no one is actively watching a show.

---

## Project Structure

```text
Apple/macos/
├── MelloTV.xcodeproj/         # Xcode project and build configurations
├── MelloTV/
│   ├── App/                   # App lifecycle, delegate, and shared environment
│   ├── Windows/               # Window controllers and locked sizing rules (1150×750)
│   ├── Navigation/            # Collapsible sidebar, tabs, and content routing
│   ├── Features/
│   │   ├── Overview/          # Server status, quick stats, and provider strip
│   │   ├── Providers/         # Provider health and live stream metrics
│   │   ├── Network/           # IP address, port info, and Tailscale connection guide
│   │   ├── Settings/          # General, Server, Updates, and TMDB catalog setup
│   │   ├── Onboarding/        # First-launch welcome window (580×640)
│   │   └── MenuBar/           # Fast menu bar status item and quick controls
│   ├── Services/              # Server process supervision, paths, preferences, updates
│   └── UI/                    # Clean satin backdrops, styles, and presentation states
├── Tests/                     # Standalone readiness probe tests
└── tools/                     # Build script that packages the server & web app into Xcode
```

---

## How to Build & Run

### In Xcode (Recommended)

1. Make sure you have **Rust** (`cargo`) and **Bun** installed.
2. Open `Apple/macos/MelloTV.xcodeproj` in Xcode.
3. Select the **Mello TV** scheme and target **My Mac**.
4. Press **Run** (`⌘R`).

> **Tip**: If you already have an installed copy of Mello TV running, quit it first so port `8080` is free.

### From the Terminal

```bash
xcodebuild -project Apple/macos/MelloTV.xcodeproj \
  -scheme "Mello TV" \
  -destination "platform=macOS" \
  CODE_SIGNING_ALLOWED=NO \
  build
```

### Standalone Readiness Tests

To run the update readiness probe tests without Xcode:

```bash
swiftc Apple/macos/MelloTV/Services/UpdateReadinessProbe.swift \
  Apple/macos/Tests/UpdateReadinessProbeTests.swift \
  -o /tmp/update-readiness-tests && /tmp/update-readiness-tests
```

---

## Key Features & How They Work

### Window Sizing
The main dashboard window opens at a locked **1150 × 750** points. You can resize it bigger, but it will never shrink below those bounds so everything stays visible and never gets cut off. The first-launch welcome screen stays compact at **580 × 640** without scrolling.

### Menu Bar & Dock Modes
You can customize where Mello TV shows up in **Settings → General**:
- **Menu bar only**: Perfect if you want it running silently in the background like a home server.
- **Dock only**: Traditional Mac app experience.
- **Both**: Best of both worlds.

### Movie & Show Metadata (TMDB)
Mello TV comes with catalog access out of the box—no API key setup is required. If you want to use your own personal TMDB account, open **Settings → Server → Catalog** and paste your key. It's stored securely in your Mac's Keychain.

### Seamless Updates
Mello TV uses Sparkle for whole-app updates. It is smart about when to install: if you or someone on your network is watching a show, it will wait until the stream is idle before restarting.

---

## Where Files Live

- **Server Data**: `~/Library/Application Support/Mello TV`
- **Logs**: `~/Library/Logs/Mello TV/MelloTV.log` (you can open this directly from the View menu or Settings)
