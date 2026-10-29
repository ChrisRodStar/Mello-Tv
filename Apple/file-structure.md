# Apple Platform File Structure

```text
Apple/
├── Packages/
│   └── MelloKit/                             # Shared local Swift package (macOS + iOS)
│       ├── Package.swift
│       ├── Sources/
│       │   └── MelloKit/
│       │       ├── Configuration/            # App-wide links & constants
│       │       │   └── AppLinks.swift
│       │       ├── DesignSystem/             # Shared styling tokens & theme
│       │       │   ├── AppearanceMode.swift
│       │       │   ├── ColorExtensions.swift
│       │       │   ├── MelloTVTheme.swift
│       │       │   └── PhosphorIcon.swift
│       │       ├── Models/                   # Shared data transfer objects
│       │       │   ├── DiscoveredServer.swift
│       │       │   ├── LibraryModels.swift
│       │       │   ├── MediaModels.swift
│       │       │   ├── ProfileModels.swift
│       │       │   ├── ServerSnapshot.swift
│       │       │   └── UpdateReadiness.swift
│       │       ├── Networking/               # API error types & network utilities
│       │       │   └── APIError.swift
│       │       ├── Persistence/              # Multi-profile UserDefaults persistence
│       │       │   └── ProfilePersistence.swift
│       │       ├── Resources/
│       │       │   └── PrivacyInfo.xcprivacy # Required reason API declarations
│       │       └── Utilities/
│       │           └── TimeFormatting.swift
│       └── Tests/
│           └── MelloKitTests/
│               └── MelloKitTests.swift
│
├── macos/                                    # macOS Server Companion App
│   ├── MelloTV.xcodeproj
│   ├── AppIcon.icns                          # Native macOS application icon
│   ├── Info.plist                            # App configuration & permissions
│   ├── MelloTVServer.entitlements            # Wasmtime executable memory permissions
│   ├── README.md                             # macOS server developer guide
│   ├── appcast.xml                           # Sparkle auto-update feed
│   ├── MelloTV/
│   │   ├── App/                              # Application entry & environment
│   │   │   ├── AppEnvironment.swift          # Dependency container & command routing
│   │   │   ├── MelloTVAppDelegate.swift      # Menu configuration & dock presence
│   │   │   └── main.swift                    # UNIX signal trapping & NSApp launch
│   │   ├── Windows/                          # AppKit Window Lifecycle
│   │   │   ├── MainWindowController.swift    # 1150 × 750 Desktop Window
│   │   │   ├── WelcomeWindowController.swift # 580 × 640 Setup Window
│   │   │   ├── WindowHostingController.swift # Isolated NSHostingView container
│   │   │   └── WindowConfiguration.swift     # Sizing single source of truth
│   │   ├── Navigation/                       # macOS Shell & Sidebar
│   │   │   ├── ContentView.swift             # Main tab switcher
│   │   │   ├── ShellView.swift               # Root sidebar + content layout
│   │   │   ├── SidebarItem.swift             # Destination enum & row item styling
│   │   │   └── SidebarView.swift             # Collapsible navigation sidebar
│   │   ├── Features/                         # Feature Domains (View + Logic)
│   │   │   ├── MenuBar/                      # Status item & dropdown
│   │   │   ├── Network/                      # IP discovery & LAN status
│   │   │   ├── Onboarding/                   # First-run welcome experience
│   │   │   ├── Overview/                     # Server portrait & status summary
│   │   │   ├── Providers/                    # Media provider strip & cards
│   │   │   └── Settings/                     # Server preferences & TMDB credentials
│   │   ├── Services/                         # Native System Integrations
│   │   │   ├── AppPreferences.swift          # App defaults persistence
│   │   │   ├── AppUpdateService.swift        # Sparkle updater delegate
│   │   │   ├── CatalogCredentialStore.swift  # Keychain API token storage
│   │   │   ├── LoginItemService.swift        # SMAppService launch-at-login
│   │   │   ├── ServerController.swift        # Background server process lifecycle
│   │   │   ├── ServerPaths.swift             # Server binary & directory paths
│   │   │   ├── ServerProcessState.swift      # Observable server run state
│   │   │   ├── ServerStatusStore.swift       # HTTP polling coordinator
│   │   │   └── UpdateReadinessProbe.swift    # Auto-update readiness checks
│   │   └── UI/                               # macOS presentation styles
│   ├── Tests/
│   │   └── UpdateReadinessProbeTests.swift   # Auto-update safety tests
│   └── tools/
│       └── prepare-xcode-bundle.sh           # Copies compiled Rust server into app bundle
│
└── ios/                                      # iOS Native Viewing Client
    ├── MelloTV.xcodeproj
    ├── README.md                             # iOS client architecture & guide
    ├── MelloTV/
    │   ├── App/                              # Application entry point
    │   │   ├── AppRootView.swift             # Top-level view router
    │   │   ├── MainTabView.swift             # Bottom tab bar navigation
    │   │   └── MelloTVApp.swift              # SwiftUI App entry point
    │   ├── Features/                         # Client Feature Domains
    │   │   ├── Catalog/                      # Media catalog & carousels
    │   │   ├── Onboarding/                   # Welcome & server connection
    │   │   ├── PersonalLibrary/              # Watchlist & Continue Watching
    │   │   ├── Playback/                     # Video player & episode navigation
    │   │   ├── Profiles/                     # Multi-user profile management
    │   │   ├── Search/                       # TMDB media search
    │   │   └── TitleDetails/                 # Detail sheets & episode lists
    │   ├── Services/                         # Client API & Networking
    │   │   ├── MelloTVAPI.swift              # HTTP client for Mello TV server
    │   │   └── ServerAddress.swift           # Server endpoint resolution & validation
    │   ├── UI/                               # Native Design System
    │   │   ├── Components/                   # Reusable buttons, cards, badges
    │   │   ├── Effects/                      # Shimmers & background effects
    │   │   └── Styles/                       # Tactile Neumorphic styling tokens
    │   └── Resources/                        # Target Resources & Manifests
    │       ├── AppIcon.icon                  # Liquid Glass icon asset
    │       ├── Assets.xcassets               # Image catalogs & colors
    │       ├── Info.plist                    # Bundle metadata & network permissions
    │       └── PrivacyInfo.xcprivacy         # App Store privacy declaration
    ├── docs/                                 # Feature specifications & research
    └── tools/                                # Verification & Fixture Scripts
        ├── PlaybackReliabilityChecks.swift   # 22 playback reliability unit tests
        ├── EpisodePromptChecks.swift         # 14 next-episode prompt logic tests
        ├── SimulatorPlaybackFixture.swift    # Realistic mock media fixture
        ├── check-playback-reliability.sh     # CLI test runner for playback checks
        ├── check-episode-prompts.sh          # CLI test runner for episode prompts
        └── prepare-simulator-fixture.py      # Simulator payload prep script
```
