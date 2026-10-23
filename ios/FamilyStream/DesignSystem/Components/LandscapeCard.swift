import SwiftUI

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
                        FamilyStreamTheme.cardBackground
                        ProgressView()
                            .tint(FamilyStreamTheme.accent)
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
            FamilyStreamTheme.secondaryBackground
            VStack(spacing: 6) {
                PhosphorIcon(item.title.type == .tv ? "television-simple" : "film-slate", size: 24)
                    .foregroundStyle(FamilyStreamTheme.muted)
                Text(item.title.title)
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(FamilyStreamTheme.muted)
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
                        .fill(item.completed ? FamilyStreamTheme.muted : FamilyStreamTheme.accent)
                        .frame(width: geometry.size.width * fraction)
                }
            }
            .frame(height: 4)
            .accessibilityHidden(true)
        }
    }
}
