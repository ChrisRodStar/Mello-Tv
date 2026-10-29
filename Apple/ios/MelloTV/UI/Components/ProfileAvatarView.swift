import SwiftUI
import MelloKit

struct ProfileAvatarView: View {
    let name: String
    let colorHex: String
    var size: CGFloat = 64
    var isSelected: Bool = false

    var body: some View {
        let cornerRadius = size * 0.18
        ZStack {
            RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                .fill(ProfileColor.color(for: colorHex))

            if let initial = name.trimmingCharacters(in: .whitespacesAndNewlines).first {
                Text(String(initial).uppercased())
                    .font(.system(size: size * 0.46, weight: .bold, design: .rounded))
                    .foregroundStyle(.white)
            } else {
                PhosphorIcon("user", size: size * 0.42, weight: .fill)
                    .foregroundStyle(.white.opacity(0.85))
            }

            if isSelected {
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .strokeBorder(MelloTVTheme.accent, lineWidth: 3)
            } else {
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 0.8)
            }
        }
        .frame(width: size, height: size)
        .shadow(color: .black.opacity(0.20), radius: 5, x: 0, y: 2)
        .accessibilityHidden(true)
    }
}
