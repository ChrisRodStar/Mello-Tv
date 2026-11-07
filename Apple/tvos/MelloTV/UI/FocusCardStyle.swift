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
                    .strokeBorder(
                        LinearGradient(
                            colors: isFocused ? [
                                Color.white.opacity(0.65),
                                Color.white.opacity(0.20)
                            ] : [
                                Color.white.opacity(0.08),
                                Color.white.opacity(0.02)
                            ],
                            startPoint: .top,
                            endPoint: .bottom
                        ),
                        lineWidth: isFocused ? 1.5 : 0.5
                    )
            )
            .shadow(
                color: isFocused ? Color.black.opacity(0.65) : Color.black.opacity(0.18),
                radius: isFocused ? 26 : 6,
                x: 0,
                y: isFocused ? 14 : 3
            )
            .scaleEffect(isFocused ? 1.05 : 1.0)
            .animation(.spring(response: 0.30, dampingFraction: 0.78), value: isFocused)
    }
}

public extension View {
    func tvCardFocusStyle(cornerRadius: CGFloat = 16) -> some View {
        modifier(TVCardFocusModifier(cornerRadius: cornerRadius))
    }
}
