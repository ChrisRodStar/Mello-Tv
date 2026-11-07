import SwiftUI
import MelloKit

/// Custom focus styling conforming to Apple TV Human Interface Guidelines.
public struct TVCardFocusModifier: ViewModifier {
    @Environment(\.isFocused) private var isFocused
    var cornerRadius: CGFloat = 16

    public func body(content: Content) -> some View {
        content
            .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .stroke(isFocused ? Color.white.opacity(0.8) : Color.clear, lineWidth: 4)
            )
            .shadow(color: isFocused ? Color.black.opacity(0.6) : Color.clear, radius: isFocused ? 24 : 0, y: isFocused ? 14 : 0)
            .scaleEffect(isFocused ? 1.06 : 1.0)
            .animation(.spring(response: 0.32, dampingFraction: 0.72), value: isFocused)
    }
}

public extension View {
    func tvCardFocusStyle(cornerRadius: CGFloat = 16) -> some View {
        modifier(TVCardFocusModifier(cornerRadius: cornerRadius))
    }
}
