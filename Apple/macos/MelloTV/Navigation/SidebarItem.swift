import SwiftUI

/// Sidebar destination item for macOS navigation.
enum SidebarItem: String, CaseIterable, Identifiable {
    case overview = "Overview"
    case users = "Users"
    case providers = "Providers"
    case network = "Network"
    case settings = "Settings"

    var id: Self { self }

    var symbol: String {
        switch self {
        case .overview: return "house"
        case .providers: return "square.stack.3d.up"
        case .network: return "network"
        case .users: return "person.2"
        case .settings: return "gearshape"
        }
    }

    var shortcut: KeyEquivalent {
        switch self {
        case .overview: return "1"
        case .providers: return "3"
        case .network: return "4"
        case .users: return "2"
        case .settings: return "7"
        }
    }
}

@MainActor
final class SidebarNavigationModel: ObservableObject {
    @Published var selection = SidebarItem.overview
    @Published var isSidebarExpanded = true
}

typealias DesktopDestination = SidebarItem
typealias DesktopNavigation = SidebarNavigationModel

