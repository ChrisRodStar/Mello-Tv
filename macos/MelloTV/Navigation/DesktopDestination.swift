import SwiftUI

enum DesktopDestination: String, CaseIterable, Identifiable {
    case overview = "Overview"
    case providers = "Providers"
    case network = "Network"
    case updates = "Updates"
    var id: Self { self }
    var symbol: String {
        switch self {
        case .overview: return "house"
        case .providers: return "square.stack.3d.up"
        case .network: return "network"
        case .updates: return "arrow.down.circle"
        }
    }
    var shortcut: KeyEquivalent {
        switch self {
        case .overview: return "1"
        case .providers: return "2"
        case .network: return "3"
        case .updates: return "4"
        }
    }
}

@MainActor
final class DesktopNavigation: ObservableObject {
    @Published var selection = DesktopDestination.overview
    @Published var isSidebarExpanded = true
}
