import MelloKit
import SwiftUI

struct SettingsProfileSection: View {
    let profile: Profile
    var onSwitchProfile: () -> Void

    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        let layout =
            dynamicTypeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 16))
            : AnyLayout(HStackLayout(alignment: .center, spacing: 16))
        return layout {
            HStack(spacing: 16) {
                ProfileAvatarView(name: profile.name, colorHex: profile.color, size: 60)
                    .clipShape(Circle())
                    .accessibilityHidden(true)
                Text(profile.name)
                    .font(.title2.weight(.semibold))
                    .tracking(-0.5)
                    .foregroundStyle(MelloTVTheme.text)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }

            Button("Switch", action: onSwitchProfile)
                .font(.subheadline.weight(.medium))
                .foregroundStyle(MelloTVTheme.textSecondary)
                .padding(.horizontal, 16)
                .frame(minHeight: 44)
                .appleNeumorphic(cornerRadius: 14, elevation: .subtle)
                .buttonStyle(CardScaleButtonStyle(scale: 0.96))
                .accessibilityLabel("Switch profile")
        }
        .padding(.vertical, 8)
    }
}
