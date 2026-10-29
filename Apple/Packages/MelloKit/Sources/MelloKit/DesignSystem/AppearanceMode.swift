import SwiftUI

public enum AppearanceMode: String, CaseIterable, Identifiable, Codable, Sendable {
    case system = "System"
    case light = "Light"
    case dark = "Dark"

    public var id: String { rawValue }

    public var iconName: String {
        switch self {
        case .system: return "circle-half"
        case .light: return "sun"
        case .dark: return "moon"
        }
    }

    public var colorScheme: ColorScheme? {
        switch self {
        case .system: return nil
        case .light: return .light
        case .dark: return .dark
        }
    }
}

