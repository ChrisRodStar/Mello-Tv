# App Store Review Guidelines: Mello TV Compliance Guide

> **Official Reference**: [Apple App Store Review Guidelines](https://developer.apple.com/app-store/review/guidelines/)  
> **Target Platforms**: iOS Client (`Apple/ios`) and macOS Companion App (`Apple/macos`)  
> **Category**: Entertainment / Media Streaming & Personal Server Client

---

## Overview

This guide curates and interprets Apple's App Store Review Guidelines specifically for **Mello TV**. Irrelevant sections (such as gambling, cryptocurrency, cellular contracts, banking, medical devices, tobacco, and alternative EU marketplaces) have been removed. 

The focus is on the requirements that directly affect a self-hosted media player: review access, playback reliability, privacy manifests, intellectual property, and monetization rules.

---

## 1. Safety & Security

### 1.1 Objectionable & User Content
- **1.1 Objectionable Content**: Apps must not display defamatory, illegal, or harmful content. Since Mello TV displays metadata and video served from user-hosted backends, the app UI itself must maintain neutral, safe defaults.
- **1.2 User-Generated Content**: While Mello TV connects to personal libraries, if community sharing or remote playlist feeds are exposed, ensure content moderation or clear personal-use boundaries are documented.
- **1.6 Data Security**:
  - Securely handle user credentials, API keys, and session tokens.
  - Sensitive server authentication tokens must be stored in the iOS/macOS Keychain (`CatalogCredentialStore`), not in plain text or insecure files.
  - Use secure transport protocols where available and respect platform networking security constraints.

---

## 2. Performance & App Completeness

### 2.1 App Completeness (Crucial for App Review)
- **Reviewer Access Requirement**: Apps submitted for review must be fully functional. Reviewers will reject apps that fail to connect or display an empty, unusable screen.
- **Action for Mello TV**:
  - App Review submission notes **must** provide access to an active demo server URL or enable the built-in simulator fixture (`SimulatorPlaybackFixture`) with preloaded catalog titles.
  - No placeholder UI ("Under Construction", "Coming Soon"), broken buttons, or mock screens in production builds.
  - Handle network disconnection, offline states, and server timeouts gracefully with user-friendly error messages and retry controls.

### 2.3 Accurate Metadata & Presentation
- **2.3.1 - 2.3.3 Screenshots & Previews**:
  - Screenshots must reflect the actual iOS app running on device hardware.
  - Do not use generic renders or desktop screenshots for iOS store listings.
  - Clearly disclose if certain features (e.g., transcoding, high-bitrate streaming) require a companion local server.
- **2.3.7 App Naming**: The name "Mello TV" must not imply official endorsement or creation by Apple (e.g., must not cause confusion with "Apple TV").

### 2.4 Hardware & Battery Performance
- **Streaming Efficiency**: Video playback must utilize native hardware-accelerated playback (`AVPlayer` / `AVPlayerViewController`).
- **Resource Management**: Avoid unbounded memory accumulation during continuous HLS or TS segment playback.
- **Background Modes**: Audio playback and background streaming sessions must declare the proper `UIBackgroundModes` (`audio`) and configure `AVAudioSessionCategoryPlayback` to support Control Center Now Playing controls and background audio properly.

### 2.5 Software Requirements & Networking
- **IPv6 Compatibility**: The app must operate seamlessly on IPv6-only networks (using DNS64/NAT64 synthesis), as mandated for all App Store submissions.
- **App Transport Security (ATS)**: For local network connections (e.g. `http://mello-server.local` or direct IP addresses), ensure appropriate `NSLocalNetworkUsageDescription` and `NSAllowsLocalNetworking` keys are declared in `Info.plist`.
- **Public APIs Only**: Use only documented, public Apple frameworks (SwiftUI, UIKit, AVFoundation, Network, Combine). Private API usage is strictly banned.

---

## 3. Business & Monetization

### 3.1 Payments & In-App Purchases
- **3.1.1 In-App Purchases & Donation Links**:
  - **Zero External Donation Links**: Direct donation buttons (e.g., "Buy Me a Coffee", PayPal, Patreon, tip jars) are strictly prohibited inside App Store binaries. Linking to third-party payment mechanisms triggers immediate rejection under Guideline 3.1.1.
  - **Permitted Alternative**: Linking to the open-source GitHub repository or project homepage for documentation and bug reporting is permitted, provided it does not direct users to external payment gateways.
- **3.1.3(a) "Reader" / Multiplatform Services**:
  - Mello TV functions as a client for user-hosted media servers. Apps that allow users to access their own content hosted on their own servers or existing subscriptions do not require In-App Purchase for core client access.

### 3.2 Business Model Integrity
- **3.2.1 Acceptable Business Models**:
  - A free client utility for self-hosted server software is a recognized, valid business model on the App Store.
  - Do not advertise features that the standalone app cannot deliver on its own without clearly explaining the server prerequisite.

---

## 4. Design & User Experience

### 4.0 Minimum Functionality
- **Native Feel**: Mello TV must provide a rich native experience tailored for iOS (fluid SwiftUI navigation, haptic feedback, tactile neumorphic buttons, responsive gestures). Repackaged web wrappers or basic RSS list views will be rejected under Guideline 4.2.
- **Orientation & Adaptation**: Ensure responsive layouts across compact (iPhone) and regular (iPad) size classes, as well as landscape playback orientation.

### 4.1 Copycats & Intellectual Property
- **Unique Branding**: Use distinctive typography, custom Phosphor iconography, and original UI layouts. Do not clone Apple TV app UI directly or copy trademarked streaming service designs.

### 4.8 Login & Authentication
- If login is optional (e.g., anonymous local streaming or local network discovery), users must be allowed to browse or configure server settings without mandatory account creation.
- If third-party sign-in is ever added, Sign in with Apple must be provided as an equivalent option if required by Guideline 4.8.

---

## 5. Legal & Intellectual Property

### 5.1 Privacy & Data Collection
- **Zero Tracking**: Mello TV does not collect, sell, or share personal user data with ad brokers.
- **Local Network Privacy**: Because Mello TV discovers local servers via mDNS/Bonjour or direct IP, `Info.plist` must include:
  - `NSLocalNetworkUsageDescription`: A clear explanation of why local network access is needed (e.g., *"Mello TV connects to your local server to stream your media library."*).
- **5.1.2 Data Use & Transparency**: App Store Connect privacy questions must accurately reflect that no analytics or advertising tracking is performed.

### 5.2 Intellectual Property & Media Rights
- **5.2.1 Ownership & Licensing**: Apps must respect copyright laws.
- **5.2.3 Audio/Video Streaming & Downloads**:
  - Mello TV is a media player for user-owned content. It must not facilitate unauthorized pirated streams or advertise illegal media downloading.
  - Documentation and metadata must present the app as a personal streaming client, similar to Plex, Infuse, or VLC.
- **5.2.5 Apple Trademarks**:
  - Never use Apple's proprietary product iconography or trademarked terms in branding.
  - The name "Mello TV" and associated icons must remain distinct from Apple's brand assets.

---

## 6. Apple Privacy Manifest (`PrivacyInfo.xcprivacy`)

Starting in iOS 17 / Spring 2024, Apple requires privacy manifests for all apps and third-party SDKs using "Required Reason APIs".

Mello TV includes `PrivacyInfo.xcprivacy` in both `Apple/Packages/MelloKit/Sources/MelloKit/Resources/` and `Apple/ios/MelloTV/Resources/`:

| API Category | Reason Code | Usage in Mello TV |
| :--- | :--- | :--- |
| **User Defaults** | `CA92.1` | Access app-specific settings, active server URL, audio preferences, and appearance mode (`MelloTVTheme`). |
| **System Boot Time** | `35F9.1` | Measure relative playback timing and session durations without tracking unique hardware identity. |

No tracking domains (`NSPrivacyTrackingDomains`) or tracking flags (`NSPrivacyTracking = false`) are used.

---

## 7. App Store Pre-Flight Checklist

Before submitting a new build to TestFlight or App Store Review:

1. **Reviewer Demo Mode**:
   - [ ] Verify test credentials or a demo server address are provided in App Review Notes.
   - [ ] Verify `prepare-simulator-fixture.py` / `SimulatorPlaybackFixture.swift` tests pass without crashing.
2. **Build & Tests**:
   - [ ] `xcodebuild -project Apple/ios/MelloTV.xcodeproj -scheme MelloTV -destination "generic/platform=iOS"` builds cleanly.
   - [ ] `./Apple/ios/tools/check-playback-reliability.sh` passes all tests.
   - [ ] `./Apple/ios/tools/check-episode-prompts.sh` passes all tests.
   - [ ] `swift test --package-path Apple/Packages/MelloKit` passes all tests.
3. **No External Donation / Payment Links**:
   - [ ] Verify no "Buy Me a Coffee", Patreon, or external donation buttons exist in Profile or Welcome screens (use GitHub project links instead).
4. **Privacy Manifests**:
   - [ ] `PrivacyInfo.xcprivacy` is included in app target bundle resources and declares `CA92.1` and `35F9.1`.
5. **Info.plist Descriptions**:
   - [ ] `NSLocalNetworkUsageDescription` is present and clearly phrased.
   - [ ] Required background playback modes (`audio`) are declared if background audio playback is enabled.
