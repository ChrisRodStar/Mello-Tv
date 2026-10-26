import AppKit
import SwiftUI

enum MelloTVTheme {
    static let workspaceColor = NSColor(srgbRed: 0.06, green: 0.06, blue: 0.07, alpha: 1)
    static let workspace = Color(nsColor: workspaceColor)
    static let content = Color(red: 0.13, green: 0.13, blue: 0.14)
    static let quietFill = Color.white.opacity(0.045)
    static let selection = Color.white.opacity(0.09)
    static let hairline = Color.white.opacity(0.08)
    static let accent = Color(red: 0.16, green: 0.59, blue: 1)
    static let onboardingBackground = NSColor(srgbRed: 0.07, green: 0.075, blue: 0.085, alpha: 1)
}
