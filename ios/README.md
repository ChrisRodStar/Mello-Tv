# Mello TV for iOS 📱

Native SwiftUI and AVKit client for **Mello TV** (`v0.1.0-alpha.1`), featuring zero-config local server discovery, seamless Tailscale roaming, background stream pre-warming, and tactile cinema playback controls.

---

## Project Structure

```text
ios/
├── MelloTV.xcodeproj/
├── MelloTV/
│   ├── App/                   # MelloTVApp entry point, AppRootView, MainTabView
│   ├── Core/
│   │   ├── API/               # MelloTVAPI client & wire models
│   │   ├── Configuration/     # Zero-config ServerAddress discovery & AppLinks
│   │   └── Persistence/       # Per-server active profile persistence
│   ├── DesignSystem/
│   │   ├── Components/        # PosterTile, LandscapeCard, ProfileAvatarView, PhosphorIcon
│   │   ├── Effects/           # Metal HeroSatin shader backdrop
│   │   └── Theme/             # MelloTVTheme colors, typography & button styles
│   ├── Features/
│   │   ├── Catalog/           # Home, Movies, TV shelves & 3D FeaturedCarouselView
│   │   ├── Onboarding/        # First-launch 3-step WelcomeOnboardingView
│   │   ├── PersonalLibrary/   # ContinueWatchingShelf, LibraryStore & ProfilePageView
│   │   ├── Playback/          # Custom AVKit PlayerView, pre-warming & progress tracking
│   │   ├── Profiles/          # WhoIsWatchingView, ProfileEditorView & taste OnboardingView
│   │   ├── Search/            # Live catalog & TMDB discovery SearchView
│   │   └── TitleDetails/      # TitleDetailSheet & EpisodeRowView
│   └── Resources/             # Info.plist, Assets.xcassets & AppIcon.icon
└── tools/                     # Offline playback reliability checks & timing scripts
```

---

## App identity

The iOS bundle identifier is `com.tinymellow.mellotv` (Tiny Mellow / Mello TV). The Home Screen name remains **Mello TV**. This is independent of the separately installed Stream TV app.

## Building & Running

Open `MelloTV.xcodeproj` in Xcode:

1. Select your physical iPhone or iPad target.
2. Under **Signing & Capabilities**, select your Apple Developer Team.
3. Build and run (`⌘R`).

### Command Line Build

```bash
xcodebuild -project ios/MelloTV.xcodeproj -scheme MelloTV \
  -configuration Debug \
  -destination 'generic/platform=iOS' \
  -allowProvisioningUpdates \
  build
```

### Offline Reliability Checks

```bash
./ios/tools/check-playback-reliability.sh
```
