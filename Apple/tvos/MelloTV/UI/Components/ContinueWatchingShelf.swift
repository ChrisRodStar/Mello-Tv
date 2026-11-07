import SwiftUI
import MelloKit

/// Horizontal shelf for in-progress items on tvOS.
/// Features 16:9 artwork cards, episode badges, playback progress bar, and Siri remote focus effects.
public struct ContinueWatchingShelf: View {
    public let items: [WatchProgress]
    public var showsHeading: Bool
    public let onPlay: (WatchProgress) -> Void
    public var onInfo: ((MediaTitle) -> Void)?

    public init(
        items: [WatchProgress],
        showsHeading: Bool = true,
        onPlay: @escaping (WatchProgress) -> Void,
        onInfo: ((MediaTitle) -> Void)? = nil
    ) {
        self.items = items
        self.showsHeading = showsHeading
        self.onPlay = onPlay
        self.onInfo = onInfo
    }

    public var body: some View {
        if !items.isEmpty {
            VStack(alignment: .leading, spacing: 18) {
                if showsHeading {
                    Text("Continue Watching")
                        .font(.title2.weight(.bold))
                        .foregroundStyle(MelloTVTheme.text)
                        .padding(.horizontal, 48)
                }

                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(alignment: .top, spacing: 32) {
                        ForEach(items, id: \.title.id) { item in
                            ContinueWatchingCard(
                                item: item,
                                onPlay: { onPlay(item) },
                                onInfo: onInfo != nil ? { onInfo?(item.title) } : nil
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

/// Focusable in-progress card displaying episode badge, remaining time, and progress bar.
public struct ContinueWatchingCard: View {
    public let item: WatchProgress
    public var width: CGFloat
    public let onPlay: () -> Void
    public var onInfo: (() -> Void)?

    public init(
        item: WatchProgress,
        width: CGFloat = 400,
        onPlay: @escaping () -> Void,
        onInfo: (() -> Void)? = nil
    ) {
        self.item = item
        self.width = width
        self.onPlay = onPlay
        self.onInfo = onInfo
    }

    @Environment(\.isFocused) private var isFocused

    private var height: CGFloat { width * (9.0 / 16.0) }
    private let cornerRadius: CGFloat = 18

    private var fraction: Double {
        guard item.position.isFinite, item.duration.isFinite, item.duration > 0 else { return 0 }
        return min(1.0, max(0.0, item.position / item.duration))
    }

    private var remainingTimeText: String {
        if item.completed {
            return item.title.type == .tv ? "Next Episode" : "Play Again"
        }
        guard item.position.isFinite, item.duration.isFinite, item.duration > 0 else { return "Resume" }
        let remainingSeconds = max(0, item.duration - item.position)
        let minutes = Int(ceil(remainingSeconds / 60))
        if minutes < 1 { return "<1m left" }
        if minutes >= 60 {
            return "\(minutes / 60)h \(minutes % 60)m left"
        }
        return "\(minutes)m left"
    }

    private var episodeBadgeText: String? {
        if item.title.type == .tv {
            return "S\(item.season) E\(item.episode)"
        }
        return nil
    }

    public var body: some View {
        Button(action: onPlay) {
            ZStack(alignment: .bottomLeading) {
                // Backdrop Artwork
                backdropArtwork
                    .frame(width: width, height: height)
                    .clipped()

                // Gradients & Frosted Glass Shading
                LinearGradient(
                    stops: [
                        .init(color: .clear, location: 0.35),
                        .init(color: .black.opacity(0.45), location: 0.65),
                        .init(color: .black.opacity(0.92), location: 1.0)
                    ],
                    startPoint: .top,
                    endPoint: .bottom
                )

                // Top Floating Season / Episode Badge
                VStack {
                    HStack {
                        if let badge = episodeBadgeText {
                            Text(badge)
                                .font(.system(size: 12, weight: .bold))
                                .foregroundStyle(Color.white)
                                .padding(.horizontal, 10)
                                .padding(.vertical, 4)
                                .background(
                                    Capsule()
                                        .fill(Color.black.opacity(0.65))
                                        .overlay(
                                            Capsule()
                                                .strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 0.75)
                                        )
                                )
                        }

                        Spacer()

                        // Center/Corner Play Indicator Icon
                        Circle()
                            .fill(isFocused ? MelloTVTheme.accent : Color.black.opacity(0.55))
                            .frame(width: 34, height: 34)
                            .overlay(
                                Image(systemName: "play.fill")
                                    .font(.system(size: 13, weight: .bold))
                                    .foregroundStyle(Color.white)
                                    .offset(x: 1)
                            )
                            .shadow(color: isFocused ? MelloTVTheme.accentGlow : Color.clear, radius: 8)
                    }
                    .padding(14)

                    Spacer()
                }

                // Bottom Content: Title, Episode Details & Progress Bar
                VStack(alignment: .leading, spacing: 8) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(item.title.title)
                            .font(.headline.weight(.bold))
                            .foregroundStyle(Color.white)
                            .lineLimit(1)

                        if !item.episodeTitle.isEmpty {
                            Text(item.episodeTitle)
                                .font(.subheadline.weight(.medium))
                                .foregroundStyle(Color.white.opacity(0.82))
                                .lineLimit(1)
                        }
                    }

                    // Progress Bar & Remaining Time
                    VStack(alignment: .leading, spacing: 6) {
                        // Playback Progress Bar
                        GeometryReader { barGeo in
                            ZStack(alignment: .leading) {
                                Capsule()
                                    .fill(Color.white.opacity(0.28))

                                Capsule()
                                    .fill(isFocused ? Color.white : MelloTVTheme.accent)
                                    .frame(width: barGeo.size.width * CGFloat(fraction))
                                    .shadow(color: isFocused ? Color.white.opacity(0.8) : Color.clear, radius: 4)
                            }
                        }
                        .frame(height: 5)

                        HStack {
                            Text(remainingTimeText)
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(Color.white.opacity(0.85))

                            Spacer()

                            if fraction > 0 {
                                Text("\(Int(fraction * 100))%")
                                    .font(.caption2.weight(.medium))
                                    .foregroundStyle(Color.white.opacity(0.65))
                            }
                        }
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 14)
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
        .accessibilityValue(remainingTimeText)
    }

    @ViewBuilder
    private var backdropArtwork: some View {
        let imageURL = item.title.backdropURL ?? item.title.posterURL
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
                Image(systemName: item.title.type == .movie ? "film" : "tv")
                    .font(.system(size: 34))
                    .foregroundStyle(MelloTVTheme.muted)

                Text(item.title.title)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(MelloTVTheme.muted)
                    .lineLimit(1)
                    .padding(.horizontal, 12)
            }
        }
        .frame(width: width, height: height)
    }

    private var accessibilityLabel: String {
        var label = "Resume \(item.title.title)"
        if item.title.type == .tv {
            label += ", Season \(item.season), Episode \(item.episode)"
            if !item.episodeTitle.isEmpty {
                label += ", \(item.episodeTitle)"
            }
        }
        return label
    }
}
