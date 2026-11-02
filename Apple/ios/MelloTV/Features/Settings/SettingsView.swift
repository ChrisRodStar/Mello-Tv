import MelloKit
import SwiftUI

struct SettingsView: View {
    let profileStore: ProfileStore
    let libraryStore: LibraryStore
    var onBrowseTitles: () -> Void
    var onSwitchProfile: () -> Void

    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @State private var presentedSheet: SettingsSheet?
    @State private var showsViewingPreferences = false

    var body: some View {
        NavigationStack {
            GeometryReader { geometry in
                let wide = geometry.size.width >= 640 && !dynamicTypeSize.isAccessibilitySize
                ScrollView {
                    settingsContent(wide: wide)
                        .padding(.horizontal, 24)
                        .padding(.top, 20)
                        .padding(.bottom, 32)
                        .frame(maxWidth: 1040)
                        .frame(maxWidth: .infinity)
                }
                .scrollBounceBehavior(.basedOnSize)
                .background { CatalogPageSurface().ignoresSafeArea() }
            }
            .toolbar(.hidden, for: .navigationBar)
            .navigationDestination(for: LibraryCollection.self) { collection in
                if let profile = profileStore.activeProfile {
                    LibraryCollectionPage(
                        collection: collection,
                        profileID: profile.id,
                        libraryStore: libraryStore,
                        onBrowseTitles: onBrowseTitles
                    )
                }
            }
        }
        .task(id: profileStore.activeProfile?.id) {
            if let profile = profileStore.activeProfile {
                await libraryStore.loadLibrary(for: profile.id)
            }
        }
        .sheet(item: $presentedSheet) { sheet in
            switch sheet {
            case .playback:
                if let profile = profileStore.activeProfile {
                    PlaybackSettingsView(profileID: profile.id)
                }
            case .recommendations:
                if let profile = profileStore.activeProfile {
                    RecommendationChoicesView(profileID: profile.id, libraryStore: libraryStore)
                }
            }
        }
        .fullScreenCover(isPresented: $showsViewingPreferences) {
            if let profile = profileStore.activeProfile {
                OnboardingView(profileStore: profileStore, profile: profile) {
                    showsViewingPreferences = false
                }
            }
        }
    }

    // MARK: - Adaptive Groups

    private func settingsContent(wide: Bool) -> some View {
        let layout =
            wide
            ? AnyLayout(HStackLayout(alignment: .top, spacing: 28))
            : AnyLayout(VStackLayout(alignment: .leading, spacing: 28))
        return layout {
            VStack(alignment: .leading, spacing: 28) {
                if let profile = profileStore.activeProfile {
                    SettingsProfileSection(profile: profile, onSwitchProfile: onSwitchProfile)
                    SettingsLibrarySection(profileID: profile.id, libraryStore: libraryStore)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            VStack(alignment: .leading, spacing: 24) {
                if profileStore.activeProfile != nil { viewingSection }
                SettingsAppearanceSection()
                SettingsServerSection(
                    name: profileStore.connectedServerName,
                    address: profileStore.connectedServerAddress
                )
                NavigationLink {
                    AboutView()
                } label: {
                    SettingsRow(title: "About", symbol: "info.circle")
                }
                .buttonStyle(.plain)
                .appleNeumorphic(cornerRadius: 20, elevation: .subtle)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    // MARK: - Viewing Preferences

    private var viewingSection: some View {
        SettingsGroup(title: "Viewing") {
            Button {
                showsViewingPreferences = true
            } label: {
                SettingsRow(title: "Genres and favorites")
            }
            SettingsRowDivider()
            Button {
                presentedSheet = .playback
            } label: {
                SettingsRow(title: "Playback")
            }
            SettingsRowDivider()
            Button {
                presentedSheet = .recommendations
            } label: {
                SettingsRow(title: "Recommendation choices")
            }
        }
        .buttonStyle(.plain)
    }
}
