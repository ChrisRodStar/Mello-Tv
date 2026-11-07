import SwiftUI

/// Top-level tvOS tab navigation bar following Apple TV Human Interface Guidelines.
public struct MainTabView: View {
    @State private var selectedTab = Tab.home

    public enum Tab: Hashable {
        case home
        case library
        case search
        case settings
    }

    public init() {}

    public var body: some View {
        TabView(selection: $selectedTab) {
            HomeView()
                .tabItem {
                    Label("Home", systemImage: "house.fill")
                }
                .tag(Tab.home)

            LibraryView()
                .tabItem {
                    Label("Library", systemImage: "square.stack.fill")
                }
                .tag(Tab.library)

            SearchView()
                .tabItem {
                    Label("Search", systemImage: "magnifyingglass")
                }
                .tag(Tab.search)

            SettingsView()
                .tabItem {
                    Label("Settings", systemImage: "gearshape.fill")
                }
                .tag(Tab.settings)
        }
    }
}
