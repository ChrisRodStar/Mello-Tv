import MelloKit
import SwiftUI

struct OnboardingNavigation: View {
    let isWelcome: Bool
    let canContinue: Bool
    var onContinue: () -> Void
    var onBack: () -> Void

    var body: some View {
        VStack(spacing: 12) {
            Button(action: onContinue) {
                HStack(spacing: 10) {
                    Text(isWelcome ? "Connect your server" : "Open library")
                    Image(systemName: "arrow.right").font(.subheadline.weight(.semibold))
                }
                .font(.headline)
                .foregroundStyle(MelloTVTheme.background)
                .frame(maxWidth: .infinity, minHeight: 56)
                .contentShape(Rectangle())
            }
            .background(MelloTVTheme.text, in: .rect(cornerRadius: 18))
            .buttonStyle(CardScaleButtonStyle(scale: 0.98))
            .disabled(!isWelcome && !canContinue)
            if isWelcome {
                Text("A Mello TV server is required.")
                    .font(.footnote)
                    .foregroundStyle(MelloTVTheme.textSecondary)
            } else {
                Button("Back", action: onBack)
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .frame(minHeight: 44)
                    .buttonStyle(.plain)
            }
        }
    }

}
