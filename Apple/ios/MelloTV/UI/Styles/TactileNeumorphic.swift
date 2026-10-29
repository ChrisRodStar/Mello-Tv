import SwiftUI
import UIKit
import MelloKit

// MARK: - UI Button & Touch Target Helpers

struct CardScaleButtonStyle: ButtonStyle {
    var scale: CGFloat = 0.97
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? scale : 1.0)
            .contentShape(Rectangle())
            .animation(.spring(response: 0.25, dampingFraction: 0.75), value: configuration.isPressed)
    }
}

extension View {
    // Apply inside Button labels: outside padding changes layout, but doesn't
    // reliably enlarge a plain button's tappable area.
    func touchTarget(minWidth: CGFloat = 44, minHeight: CGFloat = 44) -> some View {
        frame(minWidth: minWidth, minHeight: minHeight)
            .contentShape(Rectangle())
    }
}

// MARK: - Apple Neumorphic Design System (Precision Tactile Materials)

enum NeumorphicElevation {
    case subtle    // small tags, badges, chips
    case medium    // standard cards, buttons, shelf items
    case prominent // hero spotlights, modals, action sheets

    var radius: CGFloat {
        switch self {
        case .subtle: 4
        case .medium: 7
        case .prominent: 12
        }
    }

    var offset: CGFloat {
        switch self {
        case .subtle: 2.5
        case .medium: 4.5
        case .prominent: 7.0
        }
    }
}

struct AppleNeumorphicSurfaceModifier: ViewModifier {
    @Environment(\.colorScheme) private var colorScheme
    let cornerRadius: CGFloat
    let elevation: NeumorphicElevation
    var isPressed: Bool = false

    func body(content: Content) -> some View {
        let isDark = colorScheme == .dark
        let activeOffset = isPressed ? elevation.offset * 0.35 : elevation.offset
        let activeRadius = isPressed ? elevation.radius * 0.5 : elevation.radius

        content
            .background(
                surfaceGradient(isDark: isDark)
                    .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
            )
            .overlay(
                // Specular Chamfer Bevel Rim (Top-left highlight to bottom-right shade)
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .stroke(bevelGradient(isDark: isDark), lineWidth: 0.75)
            )
            .modifier(NeumorphicLightingShadows(
                isDark: isDark,
                isPressed: isPressed,
                activeOffset: activeOffset,
                activeRadius: activeRadius
            ))
    }

    private func surfaceGradient(isDark: Bool) -> LinearGradient {
        if isDark {
            return LinearGradient(
                colors: [
                    Color(hex: "212328"),
                    Color(hex: "17181c")
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
        } else {
            return LinearGradient(
                colors: [
                    Color(hex: "f7f5f0"),
                    Color(hex: "e7e4dc")
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
        }
    }

    private func bevelGradient(isDark: Bool) -> LinearGradient {
        if isDark {
            return LinearGradient(
                stops: [
                    .init(color: Color.white.opacity(0.20), location: 0.0),
                    .init(color: Color.white.opacity(0.05), location: 0.45),
                    .init(color: Color.black.opacity(0.35), location: 1.0)
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
        } else {
            return LinearGradient(
                stops: [
                    .init(color: Color.white.opacity(0.95), location: 0.0),
                    .init(color: Color.white.opacity(0.40), location: 0.45),
                    .init(color: Color(red: 170/255, green: 165/255, blue: 155/255).opacity(0.25), location: 1.0)
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
        }
    }
}

private struct NeumorphicLightingShadows: ViewModifier {
    let isDark: Bool
    let isPressed: Bool
    let activeOffset: CGFloat
    let activeRadius: CGFloat

    func body(content: Content) -> some View {
        if isDark {
            content
                // Dark Mode: Specular micro-glow on top-left edge
                .shadow(
                    color: Color.white.opacity(isPressed ? 0.02 : 0.07),
                    radius: activeRadius,
                    x: -activeOffset * 0.6,
                    y: -activeOffset * 0.6
                )
                // Dark Mode: Deep black grounding shadow
                .shadow(
                    color: Color.black.opacity(isPressed ? 0.50 : 0.80),
                    radius: activeRadius + 3,
                    x: 0,
                    y: activeOffset * 1.2
                )
        } else {
            content
                // Light Mode: Crisp contact shadow right beneath the card edge for immediate pop
                .shadow(
                    color: Color.black.opacity(isPressed ? 0.10 : 0.22),
                    radius: isPressed ? 2.5 : 4.5,
                    x: 0,
                    y: isPressed ? 1.5 : 3.0
                )
                // Light Mode: Wide, luxurious warm ambient shadow for genuine 3D float
                .shadow(
                    color: Color(red: 75/255, green: 70/255, blue: 60/255).opacity(isPressed ? 0.08 : 0.18),
                    radius: isPressed ? 6.0 : 12.0,
                    x: 0,
                    y: isPressed ? 3.0 : 7.0
                )
        }
    }
}

struct AppleNeumorphicRecessedModifier: ViewModifier {
    @Environment(\.colorScheme) private var colorScheme
    let cornerRadius: CGFloat

    func body(content: Content) -> some View {
        let isDark = colorScheme == .dark

        content
            .background(
                MelloTVTheme.sunken
                    .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
            )
            .overlay(
                // Top-leading inner shadow (depth into well)
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .stroke(
                        LinearGradient(
                            stops: [
                                .init(color: Color.black.opacity(isDark ? 0.50 : 0.16), location: 0.0),
                                .init(color: Color.clear, location: 0.55),
                                .init(color: Color.white.opacity(isDark ? 0.06 : 0.65), location: 1.0)
                            ],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        ),
                        lineWidth: 1.0
                    )
            )
            .overlay(
                // Subtle hairline border
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .stroke(MelloTVTheme.subtleBorder, lineWidth: 0.5)
            )
    }
}

// MARK: - Apple Tactile Neumorphic Button Style

struct AppleNeumorphicButtonStyle: ButtonStyle {
    var cornerRadius: CGFloat = 14
    var elevation: NeumorphicElevation = .medium

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .modifier(
                AppleNeumorphicSurfaceModifier(
                    cornerRadius: cornerRadius,
                    elevation: elevation,
                    isPressed: configuration.isPressed
                )
            )
            .scaleEffect(configuration.isPressed ? 0.98 : 1.0)
            .animation(.spring(response: 0.22, dampingFraction: 0.72), value: configuration.isPressed)
    }
}

// MARK: - View Extension Helpers

extension View {
    /// Applies an Apple-grade extruded neumorphic surface with dual directional soft shadows and chamfered bevel rim.
    func appleNeumorphic(
        cornerRadius: CGFloat = 14,
        elevation: NeumorphicElevation = .medium,
        isPressed: Bool = false
    ) -> some View {
        modifier(
            AppleNeumorphicSurfaceModifier(
                cornerRadius: cornerRadius,
                elevation: elevation,
                isPressed: isPressed
            )
        )
    }

    /// Applies an Apple-grade recessed / concave well (ideal for search inputs, segmented wells, and progress tracks).
    func appleNeumorphicRecessed(cornerRadius: CGFloat = 12) -> some View {
        modifier(AppleNeumorphicRecessedModifier(cornerRadius: cornerRadius))
    }
}

