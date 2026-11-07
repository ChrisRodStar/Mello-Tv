import SwiftUI
import MelloKit

/// 10-foot "Top 10 Today" / "Top Shows Today" horizontal shelf for tvOS.
/// Displays large typographic ranking numerals (1 to 10) layered beside focus-reactive 2:3 poster cards.
public struct NumberedShelf: View {
    public let eyebrow: String
    public let subtitle: String?
    public let titles: [MediaTitle]
    public let onSelect: (MediaTitle) -> Void

    public init(
        titles: [MediaTitle],
        eyebrow: String = "Top 10 Today",
        subtitle: String? = nil,
        onSelect: @escaping (MediaTitle) -> Void
    ) {
        self.eyebrow = eyebrow
        self.subtitle = subtitle
        self.titles = Array(titles.prefix(10))
        self.onSelect = onSelect
    }

    public init(
        section: CatalogSection,
        eyebrow: String? = nil,
        onSelect: @escaping (MediaTitle) -> Void
    ) {
        self.eyebrow = eyebrow ?? section.heading
        self.subtitle = section.reason == "liked" ? "Based on titles you liked" :
                        (section.reason == "watched" ? "Based on titles you watched" : nil)
        self.titles = Array(section.titles.prefix(10))
        self.onSelect = onSelect
    }

    public var body: some View {
        if !titles.isEmpty {
            VStack(alignment: .leading, spacing: 18) {
                // Header
                VStack(alignment: .leading, spacing: 4) {
                    Text(eyebrow)
                        .font(.title2.weight(.bold))
                        .foregroundStyle(MelloTVTheme.text)

                    if let subtitle {
                        Text(subtitle)
                            .font(.callout)
                            .foregroundStyle(MelloTVTheme.muted)
                    }
                }
                .padding(.horizontal, 48)

                // Horizontal Carousel of Numbered Tiles
                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(alignment: .bottom, spacing: 28) {
                        ForEach(Array(titles.enumerated()), id: \.element.id) { index, title in
                            NumberedPosterTile(
                                rank: index + 1,
                                title: title,
                                onSelect: { onSelect(title) }
                            )
                        }
                    }
                    .padding(.horizontal, 48)
                    .padding(.vertical, 28) // Room for focus scale elevation
                }
            }
        }
    }
}

private struct NumberedPosterTile: View {
    let rank: Int
    let title: MediaTitle
    let onSelect: () -> Void

    @Environment(\.isFocused) private var isFocused

    private let posterWidth: CGFloat = 200
    private var posterHeight: CGFloat { posterWidth * 1.5 }
    private let cornerRadius: CGFloat = 16

    var body: some View {
        Button(action: onSelect) {
            HStack(alignment: .bottom, spacing: -26) {
                // Large Typographic Ranking Digit
                Text("\(rank)")
                    .font(.system(size: 130, weight: .black, design: .rounded))
                    .monospacedDigit()
                    .foregroundStyle(
                        LinearGradient(
                            colors: isFocused ? [
                                Color.white,
                                MelloTVTheme.accent
                            ] : [
                                MelloTVTheme.textSecondary.opacity(0.40),
                                MelloTVTheme.textSecondary.opacity(0.15)
                            ],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                    )
                    .shadow(
                        color: isFocused ? MelloTVTheme.accentGlow : Color.clear,
                        radius: 12
                    )
                    .offset(y: 10)
                    .zIndex(0)
                    .allowsHitTesting(false)
                    .scaleEffect(isFocused ? 1.04 : 1.0)
                    .animation(.spring(response: 0.32, dampingFraction: 0.75), value: isFocused)

                // Focusable Poster Artwork & Metadata
                VStack(alignment: .leading, spacing: 10) {
                    posterArtwork
                        .frame(width: posterWidth, height: posterHeight)
                        .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                                .strokeBorder(
                                    isFocused ? Color.white : MelloTVTheme.subtleBorder,
                                    lineWidth: isFocused ? 4 : 0.75
                                )
                        )
                        .shadow(
                            color: isFocused ? Color.black.opacity(0.7) : Color.black.opacity(0.2),
                            radius: isFocused ? 24 : 6,
                            x: 0,
                            y: isFocused ? 14 : 3
                        )
                        .overlay {
                            if isFocused {
                                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                                    .strokeBorder(MelloTVTheme.accentGlow, lineWidth: 8)
                                    .blur(radius: 6)
                                    .opacity(0.6)
                            }
                        }
                        .scaleEffect(isFocused ? 1.06 : 1.0)
                        .animation(.spring(response: 0.32, dampingFraction: 0.75), value: isFocused)

                    // Title & Year / Rating below poster
                    VStack(alignment: .leading, spacing: 4) {
                        Text(title.title)
                            .font(.headline.weight(.semibold))
                            .foregroundStyle(isFocused ? Color.white : MelloTVTheme.text)
                            .lineLimit(1)

                        HStack(spacing: 8) {
                            if !title.year.isEmpty {
                                Text(title.year)
                                    .font(.caption2.weight(.medium))
                                    .foregroundStyle(MelloTVTheme.textSecondary)
                            }

                            if title.rating > 0 {
                                HStack(spacing: 3) {
                                    Image(systemName: "star.fill")
                                        .font(.system(size: 10))
                                        .foregroundStyle(Color.yellow)
                                    Text(String(format: "%.1f", title.rating))
                                        .font(.caption2.weight(.semibold))
                                        .foregroundStyle(MelloTVTheme.textSecondary)
                                }
                            }
                        }
                    }
                    .frame(width: posterWidth, alignment: .leading)
                }
                .zIndex(1)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel("#\(rank), \(title.title)")
        .accessibilityHint("Shows title details")
    }

    @ViewBuilder
    private var posterArtwork: some View {
        if let posterURL = title.posterURL ?? title.backdropURL {
            AsyncImage(url: posterURL) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                        .frame(width: posterWidth, height: posterHeight)
                        .clipped()
                case .failure:
                    placeholder
                case .empty:
                    ZStack {
                        MelloTVTheme.cardBackground
                        ProgressView().tint(MelloTVTheme.accent)
                    }
                @unknown default:
                    placeholder
                }
            }
        } else {
            placeholder
        }
    }

    private var placeholder: some View {
        ZStack {
            MelloTVTheme.secondaryBackground

            VStack(spacing: 8) {
                Image(systemName: title.type == .movie ? "film" : "tv")
                    .font(.system(size: 32))
                    .foregroundStyle(MelloTVTheme.muted)

                Text(title.title)
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(MelloTVTheme.muted)
                    .multilineTextAlignment(.center)
                    .lineLimit(2)
                    .padding(.horizontal, 6)
            }
        }
        .frame(width: posterWidth, height: posterHeight)
    }
}
