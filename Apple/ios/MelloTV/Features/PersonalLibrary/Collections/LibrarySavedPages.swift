import MelloKit
import SwiftUI

struct LibrarySavedPages: View {
    let titles: [MediaTitle]
    var onSelect: (MediaTitle) -> Void

    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @ScaledMetric(relativeTo: .subheadline) private var captionHeight = 44

    var body: some View {
        GeometryReader { geometry in
            let showsCaptions = geometry.size.height >= 320
            let captionSpace = showsCaptions ? captionHeight + 10 : 0
            let preferredWidth: CGFloat = dynamicTypeSize.isAccessibilitySize ? 176 : 136
            let posterWidth = min(
                preferredWidth, max(44, (geometry.size.height - 56 - captionSpace) / 1.5))
            LibraryPager(
                items: titles,
                id: \.id,
                minimumCellSize: CGSize(
                    width: posterWidth, height: posterWidth * 1.5 + captionSpace),
                maximumColumns: 6
            ) { title in
                VStack(alignment: .leading, spacing: 10) {
                    poster(title, width: posterWidth)
                    if showsCaptions {
                        Text(title.title)
                            .font(.subheadline.weight(.medium))
                            .foregroundStyle(MelloTVTheme.text)
                            .lineLimit(2)
                            .frame(height: captionHeight, alignment: .topLeading)
                            .accessibilityHidden(true)
                    }
                }
                .frame(width: posterWidth, alignment: .leading)
            }
        }
    }

    private func poster(_ title: MediaTitle, width: CGFloat) -> some View {
        Button {
            onSelect(title)
        } label: {
            AsyncImage(url: title.posterURL) { phase in
                if case .success(let image) = phase {
                    image.resizable().scaledToFill()
                } else {
                    MelloTVTheme.secondaryBackground
                        .overlay {
                            Image(systemName: "film")
                                .foregroundStyle(MelloTVTheme.textSecondary)
                        }
                }
            }
            .frame(width: width, height: width * 1.5)
            .artworkCardSurface()
            .contentShape(Rectangle())
        }
        .buttonStyle(CardScaleButtonStyle())
        .accessibilityLabel(title.title)
        .accessibilityHint("Shows title details")
    }
}
