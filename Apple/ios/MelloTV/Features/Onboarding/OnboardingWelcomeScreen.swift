import MelloKit
import SwiftUI

struct OnboardingWelcomeScreen<Navigation: View>: View {
    let wide: Bool
    let compact: Bool
    var onShowCredits: () -> Void
    let navigation: Navigation

    @ScaledMetric(relativeTo: .largeTitle) private var titleSize = 44

    // MARK: - Screen Layout

    var body: some View {
        let layout =
            wide
            ? AnyLayout(HStackLayout(alignment: .center, spacing: 56))
            : AnyLayout(VStackLayout(alignment: .leading, spacing: 28))
        return layout {
            OnboardingLibraryArtwork()
                .frame(maxWidth: .infinity)
                .frame(height: wide ? 360 : (compact ? 230 : 290))
                .overlay(alignment: .topTrailing) {
                    Button {
                        onShowCredits()
                    } label: {
                        Image(systemName: "info.circle")
                            .font(.system(size: 15, weight: .regular))
                            .foregroundStyle(MelloTVTheme.textSecondary)
                            .frame(width: 44, height: 44)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Artwork credits")
                    .accessibilityHint("View poster creators, sources, and licenses")
                }
            VStack(alignment: .leading, spacing: 28) {
                VStack(alignment: .leading, spacing: 16) {
                    Text("Your library.\nReady to watch.")
                        .font(.system(size: titleSize, weight: .bold))
                        .tracking(-1.8)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityAddTraits(.isHeader)
                    Text("Movies and series from your Mello TV server, right here on your iPhone.")
                        .font(.body)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .lineSpacing(3)
                        .fixedSize(horizontal: false, vertical: true)
                }
                VStack(alignment: .leading, spacing: 16) {
                    welcomeFeature(
                        "rectangle.stack", title: "Browse your collection",
                        detail: "Movies, series, and your saved favorites.")
                    welcomeFeature(
                        "play.circle", title: "Keep your place",
                        detail: "Continue watching with your own profile.")
                }
                if wide { navigation }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func welcomeFeature(_ symbol: String, title: String, detail: String) -> some View {
        HStack(alignment: .top, spacing: 14) {
            Image(systemName: symbol)
                .font(.title3.weight(.regular))
                .foregroundStyle(MelloTVTheme.textSecondary)
                .frame(width: 26, height: 28)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
                Text(title).font(.subheadline.weight(.semibold))
                Text(detail).font(.subheadline).foregroundStyle(MelloTVTheme.textSecondary)
            }
        }
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityElement(children: .combine)
    }

}
