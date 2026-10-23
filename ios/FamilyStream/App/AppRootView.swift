import SwiftUI

struct AppRootView: View {
    @Environment(\.scenePhase) private var scenePhase
    @AppStorage("app_appearance_mode") private var appearanceMode: AppearanceMode = .system
    @State private var profileStore = ProfileStore()
    private let api = FamilyStreamAPI(baseURL: ServerAddress.baseURL)

    var body: some View {
        ZStack {
            FamilyStreamTheme.background
                .ignoresSafeArea()

            if profileStore.isLoading && profileStore.profiles.isEmpty {
                ProgressView()
                    .tint(FamilyStreamTheme.accent)
            } else if let activeProfile = profileStore.activeProfile {
                if !activeProfile.preferences.completed || profileStore.showOnboarding {
                    OnboardingView(
                        profileStore: profileStore,
                        profile: activeProfile,
                        onCompleted: {
                            profileStore.showOnboarding = false
                        }
                    )
                } else {
                    MainTabView(api: api, profileStore: profileStore)
                }
            } else {
                WhoIsWatchingView(profileStore: profileStore)
            }
        }
        .task {
            await profileStore.loadProfiles()
        }
        .onReceive(NotificationCenter.default.publisher(for: .streamTVRecommendationsChanged)) { notification in
            guard let profileID = notification.userInfo?["profileID"] as? String,
                  let title = notification.userInfo?["favoriteTitle"] as? MediaTitle,
                  let enabled = notification.userInfo?["favoriteEnabled"] as? Bool else { return }
            profileStore.applyFavoriteChoice(profileID: profileID, title: title, enabled: enabled)
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .background { PreparedPlaybackService.shared.clearAll() }
        }
        .onChange(of: profileStore.activeProfile?.id) { _, _ in
            PreparedPlaybackService.shared.clearAll()
        }
        .preferredColorScheme(appearanceMode.colorScheme)
    }
}
