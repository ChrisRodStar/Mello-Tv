import Foundation

/// Stable identities keep each destination's state while switching tabs.
enum AppTab: String, CaseIterable, Identifiable {
    case home
    case movies
    case tv
    case settings
    case search

    static let primaryTabs: [Self] = [.home, .movies, .tv, .settings]

    var id: Self { self }

    var title: String {
        switch self {
        case .home: "Home"
        case .movies: "Movies"
        case .tv: "TV"
        case .settings: "Settings"
        case .search: "Search"
        }
    }

    var systemImage: String {
        switch self {
        case .home: "house"
        case .movies: "film"
        case .tv: "tv"
        case .settings: "gearshape"
        case .search: "magnifyingglass"
        }
    }
}
