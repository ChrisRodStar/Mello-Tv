import SwiftUI

/// Each tab owns its navigation container; tab changes don't replace the app root.
struct AppTabContent: View {
    let tab: AppTab
    let api: MelloTVAPI
    let profileStore: ProfileStore
    let libraryStore: LibraryStore
    var onBrowseTitles: () -> Void
    var onSwitchProfile: () -> Void

    var body: some View {
        switch tab {
        case .home:
            catalog(.home)
        case .movies:
            catalog(.movies)
        case .tv:
            catalog(.tv)
        case .settings:
            SettingsView(
                profileStore: profileStore,
                libraryStore: libraryStore,
                onBrowseTitles: onBrowseTitles,
                onSwitchProfile: onSwitchProfile
            )
        case .search:
            SearchView(
                api: api,
                profileID: profileStore.activeProfile?.id,
                libraryStore: libraryStore
            )
        }
    }

    private func catalog(_ category: CatalogCategory) -> some View {
        NavigationStack {
            CatalogView(
                api: api,
                category: category,
                profileID: profileStore.activeProfile?.id,
                libraryStore: libraryStore
            )
        }
    }
}
