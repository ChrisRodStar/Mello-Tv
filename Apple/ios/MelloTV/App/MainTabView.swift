import SwiftUI
import MelloKit

struct MainTabView: View {
    @State private var selection: AppTab = .home
    @State private var presentedDestination: TopDestination?
    @State private var switchProfileAfterDismissal = false
    @State private var libraryStore: LibraryStore

    let api: MelloTVAPI
    @Bindable var profileStore: ProfileStore

    init(api: MelloTVAPI, profileStore: ProfileStore) {
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
                    category: .tv,
                    profileID: activeProfileID,
                    libraryStore: libraryStore
                )
            }
            .tabItem {
                Label("TV Shows", image: selection == .tv ? "television-simple-fill" : "television-simple")
            }
            .tag(AppTab.tv)

        }
        .overlay(alignment: .top) {
            HStack {
                Button {
                    presentedDestination = .profile
                } label: {
                    if #available(iOS 26.0, *) {
                        profileIcon
                            .glassEffect(.regular.interactive(), in: Circle())
                    } else {
                        profileIcon
                            .background(.regularMaterial, in: Circle())
                            .overlay {
                                Circle().strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 0.5)
                            }
                    }
                }
                .accessibilityLabel("Profile")
                .accessibilityValue(profileStore.activeProfile?.name ?? "No profile selected")

                Spacer()

                Button {
                    presentedDestination = .search
                } label: {
                    if #available(iOS 26.0, *) {
                        searchIcon
                            .glassEffect(.regular.interactive(), in: Circle())
                    } else {
                        searchIcon
                            .background(.regularMaterial, in: Circle())
                            .overlay {
                                Circle().strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 0.5)
                            }
                    }
                }
                .accessibilityLabel("Search")
            }
            .foregroundStyle(MelloTVTheme.text)
            .buttonStyle(CardScaleButtonStyle(scale: 0.94))
            .padding(.horizontal, 16)
            .padding(.top, 2)
        }
        .sheet(item: $presentedDestination, onDismiss: {
            // The active profile owns this entire screen. Clear it only once
            // SwiftUI has finished dismissing the sheet presented by this screen.
            guard switchProfileAfterDismissal else { return }
            switchProfileAfterDismissal = false
            profileStore.switchProfile()
        }) { destination in
            Group {
                switch destination {
                case .search:
                    SearchView(api: api, profileID: activeProfileID, libraryStore: libraryStore)
                case .profile:
                    ProfilePageView(
                        profileStore: profileStore,
                        libraryStore: libraryStore,
                        onBrowseTitles: {
                            presentedDestination = nil
                            selection = .home
                        },
                        onSwitchProfile: {
                            switchProfileAfterDismissal = true
                            presentedDestination = nil
                        }
                    )
                }
            }
            .presentationDragIndicator(.visible)
        }
        .tint(MelloTVTheme.accent)
    }
    private var profileIcon: some View {
        Group {
            if let profile = profileStore.activeProfile {
                ProfileAvatarView(name: profile.name, colorHex: profile.color, size: 46)
                    .clipShape(Circle())
            } else {
                PhosphorIcon("user-circle", size: 30)
            }
        }
        .frame(width: 52, height: 52)
        .contentShape(Circle())
    }

    private var searchIcon: some View {
        PhosphorIcon("magnifying-glass", size: 25, weight: .bold)
            .frame(width: 52, height: 52)
            .contentShape(Circle())
    }
}

private enum AppTab: Hashable {
    case home
    case movies
    case tv
}

private enum TopDestination: String, Identifiable {
    case search
    case profile
    var id: String { rawValue }
}
