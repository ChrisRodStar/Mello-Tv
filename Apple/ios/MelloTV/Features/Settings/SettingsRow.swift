import MelloKit
import SwiftUI

/// Presentation shared by buttons and links; the enclosing control owns its action.
struct SettingsRow: View {
    let title: String
    var symbol: String? = nil
    var value: String? = nil

    var body: some View {
        HStack(spacing: 14) {
            if let symbol {
                Image(systemName: symbol)
                    .font(.system(size: 19, weight: .regular))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .frame(width: 24)
                    .accessibilityHidden(true)
            }
            Text(title)
                .font(.body)
                .foregroundStyle(MelloTVTheme.text)
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
            if let value {
                Text(value)
                    .font(.subheadline.monospacedDigit())
                    .foregroundStyle(MelloTVTheme.textSecondary)
            }
            Image(systemName: "chevron.right")
                .font(.caption.weight(.medium))
                .foregroundStyle(MelloTVTheme.textSecondary)
                .accessibilityHidden(true)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 16)
        .frame(minHeight: 56)
        .contentShape(Rectangle())
    }
}

struct SettingsRowDivider: View {
    var body: some View {
        Rectangle()
            .fill(MelloTVTheme.subtleBorder)
            .frame(height: 1)
            .padding(.leading, 18)
            .padding(.trailing, 18)
            .accessibilityHidden(true)
    }
}
