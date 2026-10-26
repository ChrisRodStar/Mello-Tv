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

// MARK: - Mello TV Theme (1:1 iOS Design System)

enum MelloTVTheme {
    // Core Surfaces: #eceae5 (Warm Alabaster Ceramic) <-> #121316 (Deep Obsidian Titanium)
    static let background = Color.dynamic(lightHex: "eceae5", darkHex: "121316")
    static let contentBackground = Color.dynamic(lightHex: "eceae5", darkHex: "121316")
    static let content = Color.dynamic(lightHex: "eceae5", darkHex: "121316")

    // Sunken: #e1ded7 <-> #0d0e10
    static let sunken = Color.dynamic(lightHex: "e1ded7", darkHex: "0d0e10")

    // Secondary Background: #e7e4dc <-> #1c1d22
    static let secondaryBackground = Color.dynamic(lightHex: "e7e4dc", darkHex: "1c1d22")

    // Cards: #f2f0ea <-> #1a1b20
    static let card = Color.dynamic(lightHex: "f2f0ea", darkHex: "1a1b20")
    static let cardHover = Color.dynamic(lightHex: "dedbd3", darkHex: "26282e")

    // Borders & Hairlines: #d8d4cb <-> #2a2c33
    static let border = Color.dynamic(lightHex: "d8d4cb", darkHex: "2a2c33")
    static let subtleBorder = Color.dynamic(lightHex: "000000", darkHex: "ffffff").opacity(0.08)
    static let hairline = Color.dynamic(lightHex: "d8d4cb", darkHex: "2a2c33")

    // Typography: #16171a <-> #f5f5f7
    static let text = Color.dynamic(lightHex: "16171a", darkHex: "f5f5f7")
    static let textSecondary = Color.dynamic(lightHex: "787673", darkHex: "8e9096")
    static let muted = Color.dynamic(lightHex: "888682", darkHex: "8e9096")
    static let mutedSubtle = Color.dynamic(lightHex: "a09d98", darkHex: "63656b")

    // Accent: Precision Sapphire / Studio Blue (#0066cc <-> #2997ff)
    static let accent = Color.dynamic(lightHex: "0066cc", darkHex: "2997ff")
    static let accentGlow = Color.dynamic(lightHex: "0066cc", darkHex: "2997ff").opacity(0.35)

    static let quietFill = Color.dynamic(lightHex: "000000", darkHex: "ffffff").opacity(0.045)
    static let selection = Color.dynamic(lightHex: "000000", darkHex: "ffffff").opacity(0.09)
    static let workspaceColor = NSColor(name: nil) { appearance in
        let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        return NSColor(hex: isDark ? "121316" : "eceae5")
    }
    static let workspace = Color(nsColor: workspaceColor)
    static let onboardingBackground = NSColor(srgbRed: 0.07, green: 0.075, blue: 0.085, alpha: 1)
}
