import SwiftUI
import MelloKit

/// Shared Continue Watching poster. Artwork plays; the info control opens details.
struct LandscapeCard: View {
    let item: WatchProgress
    var isPreparingPlayback = false
    var isPlaybackDisabled = false
    var onPlay: () -> Void
    var onInfo: (() -> Void)? = nil

    private var playbackLabel: String {
        item.completed ? (item.title.type == .tv ? "Next episode" : "Play again") : "Resume"
    }

    private var fraction: Double? {
        guard item.position.isFinite, item.duration.isFinite, item.duration > 0 else { return nil }
        return min(1, max(0, item.position / item.duration))
    }

    private var episodeLabel: String? {
        guard item.title.type == .tv else { return nil }
        let coordinates = "S\(item.season) · E\(item.episode)"
        return item.episodeTitle.isEmpty ? coordinates : "\(coordinates) · \(item.episodeTitle)"
    }

    private var timeLabel: String {
        if item.completed { return item.title.type == .tv ? "Episode finished" : "Finished" }
        guard let fraction else { return "In progress" }
        let minutes = ceil(item.duration * (1 - fraction) / 60)
        guard minutes < Double(Int.max) else { return "In progress" }
        return minutes > 0 ? "\(Int(minutes)) min left" : "Less than a minute left"
    }

    private var compactSummary: String {
        let episode = item.title.type == .tv ? "S\(item.season) E\(item.episode)" : nil
        let status = item.completed ? "Finished" : timeLabel.replacingOccurrences(of: " min left", with: "m left")
        return [episode, status].compactMap { $0 }.joined(separator: " · ")
    }

    private var accessibilitySummary: String {
        [episodeLabel, timeLabel].compactMap { $0 }.joined(separator: ", ")
    }

    var body: some View {
        ZStack(alignment: .topTrailing) {
            Button(action: onPlay) {
                ZStack(alignment: .bottom) {
                    artwork
                        .frame(width: 126, height: 189)
                        .clipped()

                    // Subtle bottom gradient for status readability
                    LinearGradient(
                        colors: [
                            Color.clear,
                            Color.black.opacity(0.35),
                            Color.black.opacity(0.85)
                        ],
                        startPoint: .center,
                        endPoint: .bottom
                    )
                    .allowsHitTesting(false)

                    // Play indicator
                    if isPreparingPlayback {
                        ProgressView().tint(.white)
                            .frame(maxHeight: .infinity, alignment: .center)
                    } else {
                        PhosphorIcon("play", size: 22, weight: .fill)
                            .foregroundStyle(.white)
                            .shadow(color: Color.black.opacity(0.6), radius: 4, x: 0, y: 2)
                            .frame(maxHeight: .infinity, alignment: .center)
                    }

                    VStack(spacing: 4) {
                        Text(compactSummary)
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.white)
                            .multilineTextAlignment(.center)
                            .lineLimit(1)
                            .padding(.horizontal, 6)
                            .shadow(color: Color.black.opacity(0.8), radius: 3, x: 0, y: 1)

                        progressBar
                    }
                    .padding(.bottom, 8)
                }
                .frame(width: 126, height: 189)
                .clipShape(RoundedRectangle(cornerRadius: 11, style: .continuous))
                .appleNeumorphic(cornerRadius: 11, elevation: .medium)
                .contentShape(Rectangle())
            }
            .buttonStyle(CardScaleButtonStyle(scale: 0.97))
            .disabled(isPlaybackDisabled || isPreparingPlayback)
            .accessibilityLabel("\(playbackLabel), \(item.title.title)")
            .accessibilityValue(isPreparingPlayback ? "Preparing playback" : accessibilitySummary)

            if let onInfo {
                Button(action: onInfo) {
                    PhosphorIcon("info", size: 18, weight: .fill)
                        .foregroundStyle(Color.white.opacity(0.85))
                        .shadow(color: Color.black.opacity(0.5), radius: 3, x: 0, y: 1)
                        .padding(8)
                        .touchTarget()
                }
                .buttonStyle(CardScaleButtonStyle(scale: 0.92))
                .accessibilityLabel("Details for \(item.title.title)")
            }
        }
        .frame(width: 126, height: 189)
    }

    @ViewBuilder
    private var artwork: some View {
        let imageURL = item.title.posterURL ?? item.title.backdropURL
        if let imageURL {
            AsyncImage(url: imageURL) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .scaledToFill()
                case .failure:
                    placeholder
                case .empty:
                    ZStack {
                        MelloTVTheme.cardBackground
                        ProgressView()
                            .tint(MelloTVTheme.accent)
                            .scaleEffect(0.8)
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
            VStack(spacing: 6) {
                PhosphorIcon(item.title.type == .tv ? "television-simple" : "film-slate", size: 24)
                    .foregroundStyle(MelloTVTheme.muted)
                Text(item.title.title)
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(MelloTVTheme.muted)
                    .lineLimit(2)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 6)
            }
        }
    }

    @ViewBuilder
    private var progressBar: some View {
        if let fraction {
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Rectangle().fill(.white.opacity(0.25))
                    Rectangle()
                        .fill(item.completed ? MelloTVTheme.muted : MelloTVTheme.accent)
                        .frame(width: geometry.size.width * fraction)
                }
            }
            .frame(height: 4)
            .accessibilityHidden(true)
        }
    }
}

/// Artwork-led shelf card with playback status and a separate actions menu.
struct ContinueWatchingCard: View {
    let item: WatchProgress
    var isPoster = false
    var showsDetailsButton = true
    let isPreparingPlayback: Bool
    let isPlaybackDisabled: Bool
    let onPlay: () -> Void
    let onInfo: () -> Void
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    private var fraction: Double? {
        guard item.position.isFinite, item.duration.isFinite, item.duration > 0 else { return nil }
        return min(1, max(0, item.position / item.duration))
    }

    private var remaining: String {
        if item.completed { return item.title.type == .tv ? "Next episode" : "Play again" }
        guard let fraction else { return "Resume" }
        let minutes = ceil(item.duration * (1 - fraction) / 60)
        guard minutes < Double(Int.max) else { return "Resume" }
        let value = Int(minutes)
        if value < 1 { return "<1m" }
        return value >= 60 ? "\(value / 60)h \(value % 60)m" : "\(value)m"
    }

    private var status: String {
        item.title.type == .tv && !item.completed
            ? "S\(item.season), E\(item.episode) · \(remaining)" : remaining
    }

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .bottomTrailing) {
                Button(action: onPlay) {
                    ZStack(alignment: .bottomLeading) {
                        artwork
                            .frame(width: geometry.size.width, height: geometry.size.height)
                            .clipped()
                        // Fade the blurred material in above the status row.
                        // The artwork stays sharp across the upper part of the card.
                        Rectangle()
                            .fill(.ultraThinMaterial)
                            .environment(\.colorScheme, .dark)
                            .mask {
                                LinearGradient(stops: [
                                    .init(color: .clear, location: 0.55),
                                    .init(color: .black.opacity(0.35), location: 0.72),
                                    .init(color: .black, location: 0.91)
                                ], startPoint: .top, endPoint: .bottom)
                            }
                        LinearGradient(stops: [
                            .init(color: .clear, location: 0.60),
                            .init(color: .black.opacity(0.35), location: 1)
                        ], startPoint: .top, endPoint: .bottom)

                        VStack(alignment: .leading, spacing: 9) {
                            if isPoster, item.title.type == .tv {
                                Text("S\(item.season), E\(item.episode)")
                                    .font(.caption2.weight(.semibold))
                            }
                            HStack(spacing: isPoster ? 5 : 8) {
                                if isPreparingPlayback {
                                    ProgressView().tint(.white).controlSize(.small)
                                } else {
                                    PhosphorIcon("play", size: isPoster ? 11 : 13, weight: .fill)
                                }
                                if let fraction {
                                    Capsule()
                                        .fill(.white.opacity(0.30))
                                        .overlay(alignment: .leading) {
                                            Capsule().fill(.white)
                                                .frame(width: (isPoster ? 20 : 28) * fraction)
                                        }
                                        .frame(width: isPoster ? 20 : 28, height: 4)
                                }
                                Text(isPoster ? remaining : status)
                                    .font(isPoster ? .caption2.weight(.semibold) : .caption.weight(.semibold))
                                    .lineLimit(2)
                            }
                            .foregroundStyle(.white.opacity(0.88))
                        }
                        .foregroundStyle(.white)
                        .padding(.leading, isPoster ? 10 : 14)
                        .padding(.trailing, showsDetailsButton ? (isPoster ? 40 : 48) : 10)
                        .padding(.bottom, 15)
                    }
                    .contentShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
                }
                .buttonStyle(CardScaleButtonStyle(scale: 0.98))
                .disabled(isPlaybackDisabled || isPreparingPlayback)
                .accessibilityLabel("\(item.completed ? "Play" : "Resume") \(item.title.title)")
                .accessibilityValue(isPreparingPlayback ? "Preparing playback" : status)

                if showsDetailsButton {
                    Button(action: onInfo) {
                        CardDetailsGlyph()
                    }
                    .buttonStyle(.plain)
                    .padding(.trailing, 12)
                    .padding(.bottom, 2)
                    .accessibilityLabel("Details for \(item.title.title)")
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
            .overlay(alignment: .topTrailing) {
                BrandTVMark()
                .padding(12)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
            }
            .overlay {
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .strokeBorder(.white.opacity(0.14), lineWidth: 0.5)
                    .allowsHitTesting(false)
            }
        }
        .aspectRatio(isPoster ? 2.0 / 3.0 : (dynamicTypeSize.isAccessibilitySize ? 1.15 : 1.44), contentMode: .fit)
    }

    private var artwork: some View {
        AsyncImage(url: isPoster ? (item.title.posterURL ?? item.title.backdropURL) : (item.title.backdropURL ?? item.title.posterURL)) { phase in
            if case .success(let image) = phase {
                image.resizable()
                    .aspectRatio(contentMode: isPoster ? .fit : .fill)
            } else {
                MelloTVTheme.secondaryBackground
            }
        }
        .background(MelloTVTheme.secondaryBackground)
        .accessibilityHidden(true)
    }
}

struct BrandTVMark: View {
    var body: some View {
        HStack(spacing: 2) {
            Image("brand-tv-mark")
                .resizable()
                .scaledToFit()
                .frame(width: 12, height: 11)
            Text("TV")
                .font(.system(size: 10, weight: .medium))
                .tracking(-0.2)
        }
        .frame(width: 28, height: 14)
        .foregroundStyle(.white.opacity(0.75))
        .shadow(color: .black.opacity(0.4), radius: 2, y: 1)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

/// Shared contrast treatment and alignment for details over any poster.
struct CardDetailsGlyph: View {
    var body: some View {
        Image(systemName: "ellipsis")
            .font(.system(size: 16, weight: .semibold))
            .foregroundStyle(.white)
            .frame(width: 28, height: 28)
            .background(.ultraThinMaterial, in: Circle())
            .environment(\.colorScheme, .dark)
            .background(.black.opacity(0.25), in: Circle())
            .overlay {
                Circle().strokeBorder(.white.opacity(0.15), lineWidth: 0.5)
            }
            .frame(width: 44, height: 44, alignment: .trailing)
            .contentShape(Rectangle())
    }
}
