import MelloKit
import SwiftUI

/// A single surface for related controls, with a quiet section label above it.
struct SettingsGroup<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .font(.subheadline.weight(.medium))
                .foregroundStyle(MelloTVTheme.textSecondary)
                .padding(.horizontal, 4)
                .accessibilityAddTraits(.isHeader)
            VStack(spacing: 0) {
                content
            }
            .appleNeumorphic(cornerRadius: 20, elevation: .subtle)
        }
    }
}
