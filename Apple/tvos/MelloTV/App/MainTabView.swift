import SwiftUI
import MelloKit

/// Top-level tvOS tab navigation bar following Apple TV Human Interface Guidelines.
public struct MainTabView: View {
    @State private var selectedTab: AppTab = .home

    public init() {}

    public var body: some View {
        TabView(selection: $selectedTab) {
            CatalogView(category: .home)
                .tabItem {
                    Label(AppTab.home.title, systemImage: AppTab.home.systemImage)
                }
                .tag(AppTab.home)

            CatalogView(category: .movies)
                .tabItem {
                    Label(AppTab.movies.title, systemImage: AppTab.movies.systemImage)
                }
                .tag(AppTab.movies)

            CatalogView(category: .tv)
                .tabItem {
                    Label(AppTab.tv.title, systemImage: AppTab.tv.systemImage)
                }
                .tag(AppTab.tv)

            SearchView()
                .tabItem {
                    Label(AppTab.search.title, systemImage: AppTab.search.systemImage)
                }
                .tag(AppTab.search)

            SettingsView()
                .tabItem {
                    Label(AppTab.settings.title, systemImage: AppTab.settings.systemImage)
                }
                .tag(AppTab.settings)
        }
        .background(MelloTVTheme.background.ignoresSafeArea())
    }
}
