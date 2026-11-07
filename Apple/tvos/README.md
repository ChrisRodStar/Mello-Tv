# Mello TV for Apple TV 📺

Native tvOS and AVKit client shell for **Mello TV**, engineered specifically for the 10-foot living room experience and the Siri Remote.

Following the design philosophy of **Plex, Jellyfin, and Infuse**, the Apple TV app acts as a **lean presentation shell**:
- The Mello TV backend handles stream discovery, segment decryption, relaying, and transcoding.
- The tvOS app delivers smooth 60fps focus-driven browsing, native `AVPlayerViewController` playback with system subtitle/audio controls, and zero-configuration Bonjour discovery.

---

## Project Structure

```text
Apple/tvos/
├── MelloTV.xcodeproj/         # Xcode project configured for tvOS 17.0+
├── MelloTV/
│   ├── App/                   # App entry point & standard tvOS top-tab navigation
│   │   ├── MelloTVApp.swift
│   │   └── MainTabView.swift
│   ├── Services/              # MelloTVAPI client & Bonjour mDNS discovery
│   │   ├── MelloTVAPI.swift
│   │   └── ServerAddress.swift
│   ├── UI/                    # 10-foot focusable UI components
│   │   ├── FocusCardStyle.swift   # Apple HIG parallax scale, shadow & glow
│   │   ├── PosterCard.swift       # Focus-reactive 2:3 poster card
│   │   └── HeroBillboardView.swift# Cinematic backdrop billboard
│   ├── Features/
│   │   ├── Home/              # Hero banner + horizontal category shelves
│   │   ├── Details/           # Title backdrop, metadata & season/episode carousel
│   │   ├── Library/           # Watchlist & Continue Watching grid
│   │   ├── Search/            # Native tvOS .searchable with Siri Remote keyboard
│   │   ├── Playback/          # Native AVPlayerViewController with progress sync
│   │   ├── Connection/        # Bonjour auto-discovery & manual IP fallback
│   │   └── Settings/          # Server management, app version & diagnostics
│   └── Resources/             # Info.plist & Assets.xcassets
└── README.md
```

---

## Design & HIG Principles

1. **Focus Engine First**: Fully interactive with the Siri Remote. Buttons and cards feature scale-up (1.06x), elevation shadows, and white accent outlines when focused.
2. **Native AVKit Integration**: Uses `AVPlayerViewController` so system Siri Remote gestures (click-to-pause, swipe-to-scrub, and swipe-down Info Panel for audio/subtitles) work natively.
3. **Living Room Sync**: Reports playback progress to the server's `/api/v1/playback-activity` endpoint so watching status syncs across your Mac, iPhone, and Apple TV.

---

## Building & Running

### In Xcode
1. Open `Apple/tvos/MelloTV.xcodeproj` in Xcode (`open Apple/tvos/MelloTV.xcodeproj`).
2. Select an **Apple TV 4K** simulator or your physical Apple TV.
3. Press **Run (`⌘R`)**.

### Command Line
```bash
xcodebuild -project Apple/tvos/MelloTV.xcodeproj -scheme MelloTV \
  -destination 'generic/platform=tvOS' \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO \
  build
```
