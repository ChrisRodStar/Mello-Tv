import SwiftUI

struct MainTabView: View {
    @State private var selection: AppTab = .home
    @State private var libraryStore: LibraryStore

    let api: FamilyStreamAPI
    @Bindable var profileStore: ProfileStore

    init(api: FamilyStreamAPI, profileStore: ProfileStore) {
        self.api = api
        self.profileStore = profileStore
        _libraryStore = State(initialValue: LibraryStore(api: api))

        // Dynamic Titanium Pro appearance for Tab Bar
        let appearance = UITabBarAppearance()
        appearance.configureWithDefaultBackground()
        appearance.backgroundColor = UIColor { trait in
            trait.userInterfaceStyle == .dark
                ? UIColor(hex: "121316", alpha: 0.94)
                : UIColor(hex: "eceae5", alpha: 0.94)
        }

        let itemAppearance = UITabBarItemAppearance()
        itemAppearance.normal.iconColor = UIColor { trait in
            trait.userInterfaceStyle == .dark
                ? UIColor(hex: "8e9096")
                : UIColor(hex: "828186")
        }
        itemAppearance.normal.titleTextAttributes = [
            .foregroundColor: UIColor { trait in
                trait.userInterfaceStyle == .dark
                    ? UIColor(hex: "8e9096")
                    : UIColor(hex: "828186")
            },
            .font: UIFont.systemFont(ofSize: 10, weight: .medium)
        ]
        itemAppearance.selected.iconColor = UIColor { trait in
            trait.userInterfaceStyle == .dark
                ? UIColor(hex: "2997ff")
                : UIColor(hex: "0066cc")
        }
        itemAppearance.selected.titleTextAttributes = [
            .foregroundColor: UIColor { trait in
                trait.userInterfaceStyle == .dark
                    ? UIColor(hex: "2997ff")
                    : UIColor(hex: "0066cc")
            },
            .font: UIFont.systemFont(ofSize: 10, weight: .semibold)
        ]

        appearance.stackedLayoutAppearance = itemAppearance
        appearance.inlineLayoutAppearance = itemAppearance
        appearance.compactInlineLayoutAppearance = itemAppearance

        UITabBar.appearance().standardAppearance = appearance
        UITabBar.appearance().scrollEdgeAppearance = appearance
    }

    var body: some View {
        let activeProfileID = profileStore.activeProfile?.id

        TabView(selection: $selection) {
            NavigationStack {
                CatalogView(
                    api: api,
                    category: .home,
                    profileID: activeProfileID,
                    libraryStore: libraryStore
                )
            }
            .tabItem {
                Label("Home", image: selection == .home ? "house-fill" : "house")
            }
            .tag(AppTab.home)

            NavigationStack {
                CatalogView(
                    api: api,
                    category: .movies,
                    profileID: activeProfileID,
                    libraryStore: libraryStore
                )
            }
            .tabItem {
                Label("Movies", image: selection == .movies ? "film-slate-fill" : "film-slate")
            }
            .tag(AppTab.movies)

            NavigationStack {
                CatalogView(
                    api: api,
                    category: .tv,
                    profileID: activeProfileID,
                    libraryStore: libraryStore
                )
            }
            .tabItem {
                Label("TV Shows", image: selection == .tv ? "television-simple-fill" : "television-simple")
            }
            .tag(AppTab.tv)

            SearchView(
                api: api,
                profileID: activeProfileID,
                libraryStore: libraryStore
            )
            .tabItem {
                Label("Search", image: selection == .search ? "magnifying-glass-bold" : "magnifying-glass")
            }
            .tag(AppTab.search)

            ProfilePageView(
                profileStore: profileStore,
                libraryStore: libraryStore,
                onBrowseTitles: {
                    selection = .home
                }
            )
            .tabItem {
                Label("Profile", image: selection == .profile ? "user-circle-fill" : "user-circle")
            }
            .tag(AppTab.profile)
        }
        .tint(FamilyStreamTheme.accent)
    }
}

private enum AppTab: Hashable {
    case home
    case movies
    case tv
    case search
    case profile
}
