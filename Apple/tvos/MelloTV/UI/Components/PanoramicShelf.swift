import SwiftUI
import MelloKit

/// 16:9 cinematic horizontal shelf for featured and curated collections on tvOS.
/// Features wide backdrop presentation, bottom gradient text protection, and tvOS focus parallax scale.
public struct PanoramicShelf: View {
    public let heading: String
    public let eyebrow: String?
    public let subtitle: String?
    public let titles: [MediaTitle]
    public let onSelect: (MediaTitle) -> Void

    public init(
        titles: [MediaTitle],
        heading: String,
        eyebrow: String? = nil,
        subtitle: String? = nil,
        onSelect: @escaping (MediaTitle) -> Void
    ) {
        self.titles = titles
        self.heading = heading
        self.eyebrow = eyebrow
        self.subtitle = subtitle
        self.onSelect = onSelect
    }

    public init(
        section: CatalogSection,
        eyebrow: String? = nil,
        onSelect: @escaping (MediaTitle) -> Void
    ) {
        self.titles = section.titles
        self.heading = section.heading
        self.eyebrow = eyebrow
        self.subtitle = section.reason == "liked" ? "Based on titles you liked" :
                        (section.reason == "watched" ? "Based on titles you watched" : nil)
        self.onSelect = onSelect
    }

    public var body: some View {
        if !titles.isEmpty {
            VStack(alignment: .leading, spacing: 18) {
                // Header
                VStack(alignment: .leading, spacing: 4) {
                    if let eyebrow {
                        Text(eyebrow.uppercased())
                            .font(.caption.weight(.bold))
                            .tracking(1.4)
                            .foregroundStyle(MelloTVTheme.accent)
                    }

                    Text(heading)
                        .font(.title2.weight(.bold))
                        .foregroundStyle(MelloTVTheme.text)

                    if let subtitle {
                        Text(subtitle)
                            .font(.callout)
                            .foregroundStyle(MelloTVTheme.muted)
                    }
                }
                .padding(.horizontal, 48)

                // 16:9 Cinematic Cards Row
                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(alignment: .top, spacing: 32) {
                        ForEach(titles) { title in
                            PanoramicCard(title: title) {
                                onSelect(title)
                            }
                        }
                    }
                    .padding(.horizontal, 48)
                    .padding(.vertical, 28) // Room for focus scale elevation
                }
            }
        }
    }
}

/// Focusable 16:9 widescreen card with depth gradient and metadata overlay.
public struct PanoramicCard: View {
    public let title: MediaTitle
    public var width: CGFloat
    public let action: () -> Void

    public init(title: MediaTitle, width: CGFloat = 420, action: @escaping () -> Void) {
        self.title = title
        self.width = width
        self.action = action
    }

    @Environment(\.isFocused) private var isFocused

    private var height: CGFloat { width * (9.0 / 16.0) }
    private let cornerRadius: CGFloat = 18

    public var body: some View {
        Button(action: action) {
            ZStack(alignment: .bottomLeading) {
                // Backdrop Artwork
                backdropArtwork
                    .frame(width: width, height: height)
                    .clipped()

                // Vignette & Contrast Gradients
                LinearGradient(
                    colors: [
                        Color.clear,
                        Color.black.opacity(0.35),
                        Color.black.opacity(0.88)
                    ],
                    startPoint: .center,
                    endPoint: .bottom
                )

                LinearGradient(
                    colors: [
                        Color.black.opacity(0.4),
                        Color.clear
                    ],
                    startPoint: .top,
                    endPoint: .center
                )

                // Overlay Content
                VStack(alignment: .leading, spacing: 6) {
                    Text(title.title)
                        .font(.headline.weight(.bold))
                        .foregroundStyle(Color.white)
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                        .shadow(color: Color.black.opacity(0.6), radius: 4, y: 2)

                    HStack(spacing: 10) {
                        if !title.year.isEmpty {
                            Text(title.year)
                                .font(.caption.weight(.medium))
                                .foregroundStyle(Color.white.opacity(0.85))
                        }

                        if title.rating > 0 {
                            HStack(spacing: 3) {
                                Image(systemName: "star.fill")
                                    .font(.system(size: 11))
                                    .foregroundStyle(Color.yellow)
                                Text(String(format: "%.1f", title.rating))
                                    .font(.caption.weight(.bold))
                                    .foregroundStyle(Color.white)
                            }
                        }

                        Text(title.type == .movie ? "Movie" : "Series")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(Color.white.opacity(0.9))
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(Color.white.opacity(0.20))
                            .clipShape(Capsule())
                    }
                }
                .padding(18)
            }
            .frame(width: width, height: height)
            .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .strokeBorder(
                        isFocused ? Color.white : MelloTVTheme.subtleBorder,
                        lineWidth: isFocused ? 4 : 0.75
                    )
            )
            .shadow(
                color: isFocused ? Color.black.opacity(0.7) : Color.black.opacity(0.25),
                radius: isFocused ? 26 : 8,
                x: 0,
                y: isFocused ? 14 : 4
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
        }
        .buttonStyle(.plain)
        .accessibilityLabel(accessibilityLabel)
        .accessibilityHint("Shows title details")
    }

    @ViewBuilder
    private var backdropArtwork: some View {
        let imageURL = title.backdropURL ?? title.posterURL
        if let imageURL {
            AsyncImage(url: imageURL) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                        .frame(width: width, height: height)
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
                    .font(.system(size: 34))
                    .foregroundStyle(MelloTVTheme.muted)

                Text(title.title)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(MelloTVTheme.muted)
                    .lineLimit(1)
                    .padding(.horizontal, 12)
            }
        }
        .frame(width: width, height: height)
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

/// Standalone Editorial Spotlight Card break matching iOS design for curated highlights.
public struct EditorialSpotlightCard: View {
    public let title: MediaTitle
    public let onSelect: () -> Void
    public let onPlay: () -> Void

    public init(
        title: MediaTitle,
        onSelect: @escaping () -> Void,
        onPlay: @escaping () -> Void
    ) {
        self.title = title
        self.onSelect = onSelect
        self.onPlay = onPlay
    }

    @Environment(\.isFocused) private var isFocused

    private let cardHeight: CGFloat = 340
    private let cornerRadius: CGFloat = 22

    public var body: some View {
        ZStack(alignment: .bottomLeading) {
            // Backdrop
            backdropArtwork
                .frame(maxWidth: .infinity, maxHeight: cardHeight)
                .clipped()

            // Gradients
            LinearGradient(
                stops: [
                    .init(color: .clear, location: 0.1),
                    .init(color: .black.opacity(0.4), location: 0.45),
                    .init(color: .black.opacity(0.92), location: 1.0)
                ],
                startPoint: .top,
                endPoint: .bottom
            )

            // Content
            VStack(alignment: .leading, spacing: 12) {
                Text("EDITORIAL SPOTLIGHT")
                    .font(.caption.weight(.bold))
                    .tracking(1.8)
                    .foregroundStyle(MelloTVTheme.accent)

                Text(title.title)
                    .font(.system(size: 38, weight: .bold, design: .rounded))
                    .foregroundStyle(Color.white)
                    .lineLimit(2)
                    .shadow(color: .black.opacity(0.8), radius: 6, y: 3)

                if !title.overview.isEmpty {
                    Text(title.overview)
                        .font(.body)
                        .foregroundStyle(Color.white.opacity(0.85))
                        .lineLimit(2)
                        .frame(maxWidth: 800, alignment: .leading)
                }

                // Focusable Play & Details buttons
                HStack(spacing: 20) {
                    Button(action: onPlay) {
                        HStack(spacing: 10) {
                            Image(systemName: "play.fill")
                            Text("Play")
                                .fontWeight(.semibold)
                        }
                        .padding(.horizontal, 24)
                        .padding(.vertical, 12)
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(MelloTVTheme.accent)

                    Button(action: onSelect) {
                        HStack(spacing: 8) {
                            Image(systemName: "info.circle")
                            Text("Details")
                                .fontWeight(.semibold)
                        }
                        .padding(.horizontal, 20)
                        .padding(.vertical, 12)
                    }
                    .buttonStyle(.bordered)
                }
                .padding(.top, 6)
            }
            .padding(36)
        }
        .frame(maxWidth: .infinity)
        .frame(height: cardHeight)
        .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                .strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 1)
        )
        .padding(.horizontal, 48)
    }

    @ViewBuilder
    private var backdropArtwork: some View {
        let imageURL = title.backdropURL ?? title.posterURL
        if let imageURL {
            AsyncImage(url: imageURL) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                case .failure, .empty:
                    MelloTVTheme.secondaryBackground
                @unknown default:
                    MelloTVTheme.secondaryBackground
                }
            }
        } else {
            MelloTVTheme.secondaryBackground
        }
    }
}
