import SwiftUI

/// Keeps Search detached using the system tab role supported by each iOS version.
struct AppTabContainer<Content: View>: View {
    @Binding var selection: AppTab
    private let content: (AppTab) -> Content

    init(selection: Binding<AppTab>, @ViewBuilder content: @escaping (AppTab) -> Content) {
        _selection = selection
        self.content = content
    }

    var body: some View {
        if #available(iOS 18.0, *) {
            nativeTabs
        } else {
            legacyTabs
        }
    }

    // MARK: - Native Tab Roles

    @available(iOS 18.0, *)
    private var nativeTabs: some View {
        TabView(selection: $selection) {
            ForEach(AppTab.primaryTabs) { tab in
                Tab(tab.title, systemImage: tab.systemImage, value: tab) {
                    content(tab)
                }
            }
            Tab("Search", systemImage: "magnifyingglass", value: AppTab.search, role: searchRole) {
                content(.search)
            }
        }
        .tabViewStyle(.tabBarOnly)
    }

    @available(iOS 18.0, *)
    private var searchRole: TabRole {
        if #available(iOS 27.0, *) {
            return .prominent
        }
        return .search
    }

    // MARK: - iOS 17 Compatibility

    private var legacyTabs: some View {
        TabView(selection: $selection) {
            ForEach(AppTab.allCases) { tab in
                content(tab)
                    .tabItem { Label(tab.title, systemImage: tab.systemImage) }
                    .tag(tab)
            }
        }
    }
}
