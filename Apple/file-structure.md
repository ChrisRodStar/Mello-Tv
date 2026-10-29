```text
```text
Apple/
├── Packages/
│   └── MelloKit/                             # Shared local Swift package 
│       ├── Package.swift
│       └── Sources/
│           └── MelloKit/
│               ├── Models/                   # Server snapshot & media models
│               │   ├── ServerSnapshot.swift
│               │   ├── ProviderStatus.swift
│               │   ├── MediaStatus.swift
│               │   └── Endpoint.swift
│               ├── Networking/               # Unified HTTP polling client
│               │   ├── MelloAPIClient.swift
│               │   ├── APIError.swift
│               │   └── ServerAddress.swift
│               ├── Configuration/            # Shared links & constants
│               │   ├── AppLinks.swift
│               │   └── ServerDefaults.swift
│               └── DesignSystem/             # Shared styling tokens
│                   ├── ThemeColor.swift
│                   ├── ThemeFont.swift
│                   └── BackdropTexture.swift
│
├── macOS/                                    # macOS Server Companion App
│   ├── MelloTV.xcodeproj
│   ├── Supporting Files/
│   │   ├── Info.plist
│   │   ├── MelloTVServer.entitlements        # Wasmtime executable memory permissions
│   │   ├── AppIcon.icns
│   │   └── appcast.xml                       # Sparkle update feed
│   │
│   └── Sources/
│       ├── App/                              # Application lifecycle
│       │   ├── main.swift                    # UNIX signal trapping & NSApp launch
│       │   ├── MelloTVAppDelegate.swift      # Menu configuration & dock presence
│       │   └── AppEnvironment.swift          # Dependency container & command routing
│       │
│       ├── Windows/                          # AppKit Window Lifecycle (Frame & Size Ownership)
│       │   ├── MainWindowController.swift    # 1150 × 750 Desktop Window
│       │   ├── WelcomeWindowController.swift # 580 × 640 Setup Window
│       │   ├── WindowHostingController.swift # Isolated NSHostingView container
│       │   └── WindowConfiguration.swift     # Sizing single source of truth
│       │
│       ├── Navigation/                       # macOS Shell UI
│       │   ├── SidebarView.swift             # Collapsible sidebar (was DesktopSidebarView)
│       │   ├── SidebarItem.swift             # Destination enum & row item styling
│       │   └── ShellView.swift               # Root sidebar + content view
│       │
│       ├── Features/                         # Feature Domains (View + Logic together)
│       │   ├── Overview/
│       │   │   ├── OverviewView.swift
│       │   │   ├── OverviewState.swift       # State store for Overview
│       │   │   ├── ServerPortraitView.swift  # TV box render
│       │   │   └── ProviderStripView.swift   # Bottom sources strip
│       │   ├── Providers/
│       │   │   ├── ProvidersView.swift
│       │   │   └── ProviderCardView.swift
│       │   ├── Network/
│       │   │   ├── NetworkView.swift
│       │   │   └── NetworkMonitor.swift      # Extracted from inside NetworkView.swift
│       │   ├── Settings/
│       │   │   ├── SettingsView.swift
│       │   │   ├── SettingsSection.swift
│       │   │   └── CatalogCredentialView.swift
│       │   ├── Onboarding/
│       │   │   ├── OnboardingView.swift      # Fixed 580 × 640 non-scrolling layout
│       │   │   └── OnboardingStep.swift
│       │   └── MenuBar/
│       │       ├── MenuBarController.swift   # NSStatusItem management
│       │       └── MenuBarHeaderView.swift
│       │
│       └── Services/                         # Native System Integrations
│           ├── ServerSupervisor.swift        # Bundled Rust child process management
│           ├── ServerStatusStore.swift       # HTTP polling coordinator
│           ├── TMDBKeychainStore.swift       # macOS Keychain API token storage
│           ├── LaunchAtLoginService.swift    # SMAppService wrapper
│           └── AppUpdateService.swift        # Sparkle updater delegate
│
└── iOS/                                      # iOS Native Viewing Client
    ├── MelloTV.xcodeproj
    ├── Supporting Files/
    │   ├── Info.plist
    │   └── Assets.xcassets
    └── Sources/
        ├── App/
        │   ├── MelloTVApp.swift
        │   └── MainTabView.swift
        └── Features/
            ├── Catalog/                      # Browsing & Carousel
            ├── PersonalLibrary/              # Watchlist & Continue Watching
            ├── Playback/                     # AVPlayer & Segment streaming
            ├── Search/                       # TMDB search
            └── Profiles/                     # Multi-user profile management
```
```
