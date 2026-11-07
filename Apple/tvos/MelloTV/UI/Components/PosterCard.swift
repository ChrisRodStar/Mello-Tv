import SwiftUI
import MelloKit

/// Focusable 2:3 poster card for tvOS conforming to Apple TV 10-foot design standards.
/// Features smooth focus scaling, titanium theme borders, dynamic metadata, and high-contrast typography.
public struct PosterCard: View {
    public let title: MediaTitle
    public var width: CGFloat
    public var showMetadata: Bool
    public let action: () -> Void

    public init(
        title: MediaTitle,
        width: CGFloat = 220,
        showMetadata: Bool = true,
        action: @escaping () -> Void
    ) {
        self.title = title
        self.width = width
        self.showMetadata = showMetadata
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            PosterCardContent(
                title: title,
                width: width,
                showMetadata: showMetadata
            )
        }
        .buttonStyle(.plain)
        .accessibilityLabel(accessibilityLabel)
    }

    private var accessibilityLabel: String {
        var parts = [title.title]
        if !title.year.isEmpty { parts.append(title.year) }
        parts.append(title.type == .movie ? "Movie" : "Series")
        if title.rating > 0 {
            parts.append(String(format: "Rating %.1f", title.rating))
        }
        return parts.joined(separator: ", ")
    }
}

private struct PosterCardContent: View {
    let title: MediaTitle
    let width: CGFloat
    let showMetadata: Bool

    @Environment(\.isFocused) private var isFocused

    private var posterHeight: CGFloat { width * 1.5 }
    private var cornerRadius: CGFloat { 16 }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // 2:3 Poster Surface
            ZStack {
                posterArtwork
            }
            .frame(width: width, height: posterHeight)
            .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .strokeBorder(
                        isFocused ? Color.white : MelloTVTheme.subtleBorder,
                        lineWidth: isFocused ? 4 : 0.75
                    )
            )
            .shadow(
                color: isFocused ? Color.black.opacity(0.65) : Color.black.opacity(0.2),
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

            // Metadata Below Poster
            if showMetadata {
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
                                    .font(.system(size: 11))
                                    .foregroundStyle(Color.yellow)
                                Text(String(format: "%.1f", title.rating))
                                    .font(.caption2.weight(.semibold))
                                    .foregroundStyle(MelloTVTheme.textSecondary)
                            }
                        }

                        Text(title.type == .movie ? "Movie" : "Series")
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(MelloTVTheme.muted)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(MelloTVTheme.quietFill)
                            .clipShape(Capsule())
                    }
                }
                .frame(width: width, alignment: .leading)
            }
        }
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
                        .frame(width: width, height: posterHeight)
                        .clipped()
                case .failure:
                    placeholder
                case .empty:
                    ZStack {
                        MelloTVTheme.cardBackground
                        ProgressView()
                            .tint(MelloTVTheme.accent)
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

            VStack(spacing: 10) {
                Image(systemName: title.type == .movie ? "film" : "tv")
                    .font(.system(size: 38))
                    .foregroundStyle(MelloTVTheme.muted)

                Text(title.title)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(MelloTVTheme.muted)
                    .multilineTextAlignment(.center)
                    .lineLimit(2)
                    .padding(.horizontal, 10)
            }
        }
        .frame(width: width, height: posterHeight)
    }
}
