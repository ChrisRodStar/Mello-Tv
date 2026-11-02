import MelloKit
import SwiftUI

/// Owns the shared library and tab selection; profile collections push inside Settings.
struct MainTabView: View {
    let api: MelloTVAPI
    @Bindable var profileStore: ProfileStore

    @State private var selection: AppTab = .home
    @State private var libraryStore: LibraryStore

    init(api: MelloTVAPI, profileStore: ProfileStore) {
        self.api = api
        self.profileStore = profileStore
        _libraryStore = State(initialValue: LibraryStore(api: api))
    }

    var body: some View {
        AppTabContainer(selection: $selection) { tab in
            AppTabContent(
                tab: tab,
                api: api,
                profileStore: profileStore,
                libraryStore: libraryStore,
                onBrowseTitles: { selection = .home },
                onSwitchProfile: { profileStore.switchProfile() }
            )
        }
        .tint(MelloTVTheme.accent)
    }

}
