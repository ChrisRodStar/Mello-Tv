import Foundation

/// Stable identities keep each destination's state while switching tabs on tvOS.
public enum AppTab: String, CaseIterable, Identifiable, Hashable {
    case home
    case movies
    case tv
    case search
    case settings

    public static let primaryTabs: [Self] = [.home, .movies, .tv, .search, .settings]

    public var id: Self { self }

    public var title: String {
        switch self {
        case .home: "Home"
        case .movies: "Movies"
        case .tv: "TV"
        case .search: "Search"
        case .settings: "Settings"
        }
    }

    public var systemImage: String {
        switch self {
        case .home: "house"
        case .movies: "film"
        case .tv: "tv"
        case .search: "magnifyingglass"
        case .settings: "gearshape"
        }
    }
}
