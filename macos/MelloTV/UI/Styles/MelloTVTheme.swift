import AppKit
import SwiftUI

// MARK: - Appearance Mode

enum AppearanceMode: String, CaseIterable, Identifiable, Codable {
    case system = "System"
    case light = "Light"
    case dark = "Dark"

    var id: String { rawValue }

    var iconName: String {
        switch self {
        case .system: return "circle.righthalf.filled"
        case .light: return "sun.max.fill"
        case .dark: return "moon.fill"
        }
    }

    var colorScheme: ColorScheme? {
        switch self {
        case .system: return nil
        case .light: return .light
        case .dark: return .dark
        }
    }
}

// MARK: - Dynamic Color Helpers

extension Color {
    static func dynamic(lightHex: String, darkHex: String) -> Color {
        Color(nsColor: NSColor(name: nil, dynamicProvider: { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            return NSColor(hex: isDark ? darkHex : lightHex)
        }))
    }
}

extension NSColor {
    convenience init(hex: String, alpha: CGFloat = 1.0) {
        var clean = hex.trimmingCharacters(in: .whitespacesAndNewlines).uppercased()
        if clean.hasPrefix("#") { clean.remove(at: clean.startIndex) }
        var rgbValue: UInt64 = 0
        Scanner(string: clean).scanHexInt64(&rgbValue)
        let r = CGFloat((rgbValue & 0xFF0000) >> 16) / 255.0
        let g = CGFloat((rgbValue & 0x00FF00) >> 8) / 255.0
        let b = CGFloat(rgbValue & 0x0000FF) / 255.0
        self.init(srgbRed: r, green: g, blue: b, alpha: alpha)
    }
}

// MARK: - Mello TV Theme (Titanium White <-> Deep Obsidian Titanium)

enum MelloTVTheme {
    // Canvas / Content Surfaces: #FFFFFF (Crisp Titanium White) <-> #121316 (Deep Obsidian Titanium)
    static let background = Color.dynamic(lightHex: "FFFFFF", darkHex: "121316")
    static let contentBackground = Color.dynamic(lightHex: "FFFFFF", darkHex: "121316")
    static let content = Color.dynamic(lightHex: "FFFFFF", darkHex: "121316")

    // Sunken Tracks / Pickers: #EFEFF2 <-> #0D0E10
    static let sunken = Color.dynamic(lightHex: "EFEFF2", darkHex: "0D0E10")

    // Secondary Background: #F5F5F7 <-> #1C1D22
    static let secondaryBackground = Color.dynamic(lightHex: "F5F5F7", darkHex: "1C1D22")

    // Card Surfaces: #FFFFFF (with crisp border & shadow) <-> #1A1B20
    static let card = Color.dynamic(lightHex: "FFFFFF", darkHex: "1A1B20")
    static let cardHover = Color.dynamic(lightHex: "F2F2F5", darkHex: "26282E")

    // Borders & Hairlines: #E2E2E7 (Crisp Apple Titanium Border) <-> #2A2C33
    static let border = Color.dynamic(lightHex: "E2E2E7", darkHex: "2A2C33")
    static let subtleBorder = Color.dynamic(lightHex: "EAEAEE", darkHex: "2A2C33")
    static let hairline = Color.dynamic(lightHex: "E2E2E7", darkHex: "2A2C33")

    // Typography: #111113 (Deep Black) <-> #F5F5F7
    static let text = Color.dynamic(lightHex: "111113", darkHex: "F5F5F7")
    static let textSecondary = Color.dynamic(lightHex: "6E6E73", darkHex: "8E9096")
    static let muted = Color.dynamic(lightHex: "86868B", darkHex: "8E9096")
    static let mutedSubtle = Color.dynamic(lightHex: "AEAEB2", darkHex: "63656B")

    // Accent: Precision Sapphire / Studio Blue (#0066CC <-> #2997FF)
    static let accent = Color.dynamic(lightHex: "0066CC", darkHex: "2997FF")
    static let accentGlow = Color.dynamic(lightHex: "0066CC", darkHex: "2997FF").opacity(0.35)

    static let quietFill = Color.dynamic(lightHex: "000000", darkHex: "FFFFFF").opacity(0.05)
    static let selection = Color.dynamic(lightHex: "000000", darkHex: "FFFFFF").opacity(0.08)

    // Shell Workspace: Cool Titanium Silver (#F2F2F7) <-> #121316
    static let workspaceColor = NSColor(name: nil) { appearance in
        let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        return NSColor(hex: isDark ? "121316" : "F2F2F7")
    }
    static let workspace = Color(nsColor: workspaceColor)
    static let onboardingBackground = NSColor(srgbRed: 0.07, green: 0.075, blue: 0.085, alpha: 1)
}
