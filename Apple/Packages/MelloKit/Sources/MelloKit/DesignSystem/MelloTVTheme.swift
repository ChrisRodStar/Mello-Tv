import SwiftUI

#if canImport(AppKit)
import AppKit
#endif

/// Shared titanium finishes: silver-gray in light appearance, graphite in dark.
/// Blue remains the interaction accent; surface depth comes from neutral lighting.
public enum MelloTVTheme {
    // MARK: - Surfaces
    public static let background = Color.dynamic(lightHex: "e6e8ea", darkHex: "121316")
    public static let contentBackground = background
    public static let content = background
    public static let sunken = Color.dynamic(lightHex: "d8dce0", darkHex: "0d0e10")
    public static let secondaryBackground = Color.dynamic(lightHex: "dfe2e5", darkHex: "1c1d22")
    public static let card = Color.dynamic(lightHex: "f0f2f4", darkHex: "1a1b20")
    public static let cardBackground = card
    public static let cardHover = Color.dynamic(lightHex: "e2e5e8", darkHex: "26282e")

    // Directional light across a satin metal surface, shared by raised controls.
    public static let surfaceHighlight = Color.dynamic(lightHex: "f7f8f9", darkHex: "212328")
    public static let surfaceShade = Color.dynamic(lightHex: "dce0e4", darkHex: "17181c")
    public static let ambientShadow = Color.dynamic(lightHex: "48515b", darkHex: "000000")

    // MARK: - Borders & Hairlines
    public static let border = Color.dynamic(lightHex: "c8cdd3", darkHex: "2a2c33")
    public static let subtleBorder = Color.dynamic(lightHex: "000000", lightAlpha: 0.05, darkHex: "ffffff", darkAlpha: 0.08)
    public static let hairline = border

    // MARK: - Typography
    public static let text = Color.dynamic(lightHex: "16171a", darkHex: "f5f5f7")
    public static let textSecondary = Color.dynamic(lightHex: "60666e", darkHex: "8e9096")
    public static let muted = Color.dynamic(lightHex: "707780", darkHex: "8e9096")
    public static let mutedSubtle = Color.dynamic(lightHex: "8b929a", darkHex: "63656b")

    // MARK: - Accents & Interactive Fills
    public static let accent = Color.dynamic(lightHex: "0066cc", darkHex: "2997ff")
    public static let accentGlow = Color.dynamic(lightHex: "0066cc", lightAlpha: 0.25, darkHex: "2997ff", darkAlpha: 0.35)
    public static let quietFill = Color.dynamic(lightHex: "000000", lightAlpha: 0.05, darkHex: "ffffff", darkAlpha: 0.07)
    public static let selection = Color.dynamic(lightHex: "000000", lightAlpha: 0.08, darkHex: "ffffff", darkAlpha: 0.12)

    // MARK: - Platform-Specific Backgrounds
    #if canImport(AppKit)
    public static let workspaceColor = NSColor(name: nil) { appearance in
        let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        return NSColor(hex: isDark ? "0d0e10" : "d8dce0")
    }
    public static let workspace = Color(nsColor: workspaceColor)
    public static let onboardingBackground = NSColor(srgbRed: 0.07, green: 0.075, blue: 0.085, alpha: 1)
    #endif
}
