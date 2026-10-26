import SwiftUI

struct PosterTile: View {
    let title: MediaTitle
    var action: (() -> Void)? = nil
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    private var width: CGFloat { dynamicTypeSize.isAccessibilitySize ? 176 : 136 }

    var body: some View {
        if let action {
            Button(action: action) {
                content.contentShape(Rectangle())
            }
            .buttonStyle(CardScaleButtonStyle())
            .accessibilityLabel(accessibilityTitle)
            .accessibilityHint("Shows title details")
        } else {
            content
                .accessibilityLabel(accessibilityTitle)
        }
    }

    private var content: some View {
        poster
            .frame(width: width, height: width * 1.5)
            .artworkCardSurface()
    }

    @ViewBuilder
    private var poster: some View {
        if let posterURL = title.posterURL {
            AsyncImage(url: posterURL) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .scaledToFill()
                case .failure:
                    posterPlaceholder
                case .empty:
                    MelloTVTheme.cardBackground
                @unknown default:
                    posterPlaceholder
                }
            }
        } else {
            posterPlaceholder
        }
    }

    private var posterPlaceholder: some View {
        ZStack {
            MelloTVTheme.secondaryBackground

            VStack(spacing: 8) {
                PhosphorIcon(title.type == .movie ? "film-slate" : "television-simple", size: 24)
                    .foregroundStyle(MelloTVTheme.muted)

                Text(title.title)
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .multilineTextAlignment(.center)
                    .lineLimit(3)
                    .padding(.horizontal, 8)
            }
        }
    }

    private var accessibilityTitle: String {
        var parts = [title.title]
        if !title.year.isEmpty { parts.append(title.year) }
        if title.rating > 0 {
            let rating = title.rating.formatted(.number.precision(.fractionLength(1)))
            parts.append("Rated \(rating)")
        }
        return parts.joined(separator: ", ")
    }
}

/// A restrained edge shared by artwork cards across browsing, search, and details.
extension View {
    func artworkCardSurface(cornerRadius: CGFloat = 16) -> some View {
        overlay(alignment: .topTrailing) {
            BrandTVMark()
                .padding(10)
        }
        .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 0.5)
                    .allowsHitTesting(false)
            }
            .shadow(color: .black.opacity(0.10), radius: 6, y: 3)
    }
}
