import SwiftUI
import MelloKit

/// Shared page geometry keeps the composition centered inside the content pane.
struct DesktopPage<Content: View>: View {
    var maxWidth: CGFloat = 820
    @ViewBuilder var content: Content

    var body: some View {
        content
            .frame(maxWidth: maxWidth)
            .padding(36)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
    }
}

struct DesktopPageHeading: View {
    let title: String
    var detail: String? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.system(size: 34, weight: .bold))
                .tracking(-1.1)
                .foregroundStyle(MelloTVTheme.text)
                .accessibilityAddTraits(.isHeader)
            if let detail {
                Text(detail)
                    .font(.system(size: 13))
                    .foregroundStyle(MelloTVTheme.textSecondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct StudioSurface: ViewModifier {
    var radius: CGFloat = 20

    func body(content: Content) -> some View {
        content
            .background(MelloTVTheme.card, in: RoundedRectangle(cornerRadius: radius, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .strokeBorder(MelloTVTheme.border, lineWidth: 1)
            }
            .shadow(color: Color.black.opacity(0.08), radius: 8, y: 3)
    }
}

struct StudioButtonStyle: ButtonStyle {
    var primary = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 13, weight: .semibold))
            .padding(.horizontal, 18)
            .frame(height: 38)
            .foregroundStyle(primary ? Color.white : MelloTVTheme.text)
            .background(primary ? MelloTVTheme.accent : MelloTVTheme.sunken, in: Capsule())
            .overlay {
                Capsule().strokeBorder(primary ? Color.clear : MelloTVTheme.border, lineWidth: 1)
            }
            .opacity(configuration.isPressed ? 0.65 : 1)
    }
}
