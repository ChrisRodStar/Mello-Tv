import SwiftUI

struct EpisodeRowView: View {
    let episode: Episode
    let progress: WatchProgress?
    let isWatched: Bool
    var isNextUp = false
    let hasManualWatchedMark: Bool
    var onPlay: () -> Void
    var onRewatch: () -> Void
    var onSetWatched: (Bool) -> Void

    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var isExpanded = false

    private var progressFraction: Double? {
        if isWatched { return 1 }
        guard let progress, progress.duration.isFinite, progress.position.isFinite,
              progress.duration > 0, progress.position > 0 else { return nil }
        return min(1.0, max(0.0, progress.position / progress.duration))
    }

    private var episodeMetadata: String {
        var values: [String] = []
        if isWatched {
            values.append("Watched")
        } else if let progress, progressFraction != nil {
            let minutes = max(1, ceil((progress.duration - progress.position) / 60))
            values.append("\(minutes.formatted(.number.precision(.fractionLength(0)))) min remaining")
        } else if let runtime = episode.runtime, runtime > 0 {
            values.append("\(runtime) min")
        }
        return values.joined(separator: "  ·  ")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if dynamicTypeSize.isAccessibilitySize {
                VStack(alignment: .leading, spacing: 12) {
                    preview
                    episodeCopy
                }
            } else {
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .top, spacing: 12) {
                        preview.frame(width: 144)
                        episodeCopy.frame(minWidth: 140, maxWidth: .infinity, alignment: .leading)
                    }
                    VStack(alignment: .leading, spacing: 12) {
                        preview
                        episodeCopy
                    }
                }
            }
            if isExpanded, !episode.overview.isEmpty {
                Text(episode.overview)
                    .font(.subheadline)
                    .lineSpacing(4)
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .transition(.opacity)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(isNextUp ? MelloTVTheme.accent.opacity(0.08) : MelloTVTheme.cardBackground.opacity(0.45),
                    in: RoundedRectangle(cornerRadius: 20))
        .overlay {
            RoundedRectangle(cornerRadius: 20)
                .strokeBorder(isNextUp ? MelloTVTheme.accent.opacity(0.22) : MelloTVTheme.border.opacity(0.35), lineWidth: 0.5)
        }
        .contextMenu { episodeActions }
    }

    @ViewBuilder
    private var episodeActions: some View {
        Button(action: onRewatch) {
            Label(isWatched ? "Rewatch from beginning" : "Play from beginning", image: "arrow-counter-clockwise")
        }
        if hasManualWatchedMark || !isWatched {
            Button {
                onSetWatched(!hasManualWatchedMark)
            } label: {
                Label(hasManualWatchedMark ? "Undo watched mark on this iPhone" : "Mark as watched on this iPhone",
                      image: hasManualWatchedMark ? "arrow-u-up-left" : "check")
            }
        }
    }

    private var preview: some View {
        Button(action: onPlay) {
            Color.clear
                .aspectRatio(16.0 / 9, contentMode: .fit)
                .overlay {
                    thumbnail
                        .saturation(isWatched ? 0.65 : 1)
                        .overlay(.black.opacity(0.12))
                }
                .overlay {
                    PhosphorIcon("play", size: 20, weight: .fill)
                        .foregroundStyle(.white)
                        .accessibilityHidden(true)
                        .allowsHitTesting(false)
                }
                .overlay(alignment: .bottomLeading) {
                    if let fraction = progressFraction {
                        GeometryReader { geometry in
                            ZStack(alignment: .leading) {
                                Rectangle().fill(.black.opacity(0.55))
                                Rectangle()
                                    .fill(isWatched ? MelloTVTheme.muted : MelloTVTheme.accent)
                                    .frame(width: geometry.size.width * fraction)
                            }
                        }
                        .frame(height: 4)
                        .allowsHitTesting(false)
                    }
                }
                .clipShape(.rect(cornerRadius: 12))
                .contentShape(Rectangle())
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.98))
        .accessibilityLabel("Play episode \(episode.number), \(episode.name)")
        .accessibilityValue(episodeMetadata)
        .accessibilityActions { episodeActions }
    }

    private var episodeCopy: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button(action: onPlay) {
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 6) {
                        Text("EPISODE \(episode.number)")
                        if isNextUp {
                            Circle().fill(MelloTVTheme.accent).frame(width: 4, height: 4)
                            Text("UP NEXT")
                        }
                    }
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(isNextUp ? MelloTVTheme.accent : MelloTVTheme.muted)
                    .fixedSize(horizontal: false, vertical: true)
                    Text(episode.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "Episode \(episode.number)" : episode.name)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(MelloTVTheme.text)
                        .fixedSize(horizontal: false, vertical: true)
                    if !episodeMetadata.isEmpty {
                        Text(episodeMetadata)
                            .font(.caption)
                            .foregroundStyle(MelloTVTheme.muted)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                .multilineTextAlignment(.leading)
                .frame(maxWidth: .infinity, minHeight: 44, alignment: .topLeading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Play episode \(episode.number), \(episode.name)")
            .accessibilityValue(episodeMetadata)
            .accessibilityActions { episodeActions }

            HStack(spacing: 8) {
                if !episode.overview.isEmpty {
                    Button {
                        withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.2)) { isExpanded.toggle() }
                    } label: {
                        HStack(spacing: 5) {
                            Text(isExpanded ? "Less" : "Synopsis")
                            Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                                .font(.system(size: 9, weight: .semibold))
                        }
                        .font(.caption)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .frame(minHeight: 44)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("\(isExpanded ? "Collapse" : "Expand") synopsis for episode \(episode.number)")
                    .accessibilityValue(isExpanded ? "Expanded" : "Collapsed")
                }
                Spacer(minLength: 0)
                Menu { episodeActions } label: {
                    Image(systemName: "ellipsis")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .frame(width: 44, height: 44)
                        .contentShape(Circle())
                }
                .accessibilityLabel("Options for episode \(episode.number)")
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var thumbnail: some View {
        GeometryReader { geometry in
            Group {
                if let stillURL = episode.stillURL {
                    AsyncImage(url: stillURL) { phase in
                        switch phase {
                        case .success(let image):
                            image.resizable().scaledToFill()
                        case .empty:
                            MelloTVTheme.secondaryBackground
                                .overlay(ProgressView().tint(MelloTVTheme.accent))
                        case .failure:
                            placeholderThumbnail
                        @unknown default:
                            placeholderThumbnail
                        }
                    }
                } else {
                    placeholderThumbnail
                }
            }
            .frame(width: geometry.size.width, height: geometry.size.height)
            .clipped()
        }
        .accessibilityHidden(true)
    }

    private var placeholderThumbnail: some View {
        MelloTVTheme.secondaryBackground
            .overlay {
                PhosphorIcon("television-simple", size: 24)
                    .foregroundStyle(MelloTVTheme.muted)
            }
    }
}
