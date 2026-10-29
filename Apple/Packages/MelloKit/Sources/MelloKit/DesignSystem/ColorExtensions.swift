import SwiftUI

#if canImport(AppKit)
import AppKit
#endif

#if canImport(UIKit)
import UIKit
#endif

public extension Color {
    init(hex: String) {
        let cleanHex = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var int: UInt64 = 0
        Scanner(string: cleanHex).scanHexInt64(&int)
        let a, r, g, b: UInt64
        switch cleanHex.count {
        case 3: // RGB (12-bit)
            (a, r, g, b) = (255, (int >> 8) * 17, (int >> 4 & 0xF) * 17, (int & 0xF) * 17)
        case 6: // RGB (24-bit)
            (a, r, g, b) = (255, int >> 16, int >> 8 & 0xFF, int & 0xFF)
        case 8: // ARGB (32-bit)
            (a, r, g, b) = (int >> 24, int >> 16 & 0xFF, int >> 8 & 0xFF, int & 0xFF)
        default:
            (a, r, g, b) = (255, 100, 100, 100)
        }
        self.init(
            .sRGB,
            red: Double(r) / 255.0,
            green: Double(g) / 255.0,
            blue: Double(b) / 255.0,
            opacity: Double(a) / 255.0
        )
    }

    static func dynamic(lightHex: String, darkHex: String) -> Color {
        #if canImport(AppKit)
        return Color(nsColor: NSColor(name: nil, dynamicProvider: { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            return NSColor(hex: isDark ? darkHex : lightHex)
        }))
        #elseif canImport(UIKit)
        return Color(uiColor: UIColor { traitCollection in
            let isDark = traitCollection.userInterfaceStyle == .dark
            return UIColor(hex: isDark ? darkHex : lightHex)
        })
        #else
        return Color(hex: lightHex)
        #endif
    }
}

#if canImport(AppKit)
public extension NSColor {
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
#endif

#if canImport(UIKit)
public extension UIColor {
    convenience init(hex: String, alpha: CGFloat = 1.0) {
        var clean = hex.trimmingCharacters(in: .whitespacesAndNewlines).uppercased()
        if clean.hasPrefix("#") { clean.remove(at: clean.startIndex) }
        var rgbValue: UInt64 = 0
        Scanner(string: clean).scanHexInt64(&rgbValue)
        let r = CGFloat((rgbValue & 0xFF0000) >> 16) / 255.0
        let g = CGFloat((rgbValue & 0x00FF00) >> 8) / 255.0
        let b = CGFloat(rgbValue & 0x0000FF) / 255.0
        self.init(red: r, green: g, blue: b, alpha: alpha)
    }
}
#endif

