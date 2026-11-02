import MelloKit
import SwiftUI

/// A compact resume target plus a separate, full-size details button.
struct LibraryProgressRow: View {
    let item: WatchProgress
    let showsDate: Bool
    let isPreparingPlayback: Bool
    let isPlaybackDisabled: Bool
    var onPlay: () -> Void
    var onInfo: () -> Void

    private var fraction: Double? {
        guard item.duration.isFinite, item.position.isFinite, item.duration > 0 else { return nil }
        return min(1, max(0, item.position / item.duration))
    }

    private var detail: String {
        var parts: [String] = []
        if item.title.type == .tv { parts.append("S\(item.season) · E\(item.episode)") }
        if item.completed {
            parts.append(
                showsDate ? "Watched" : (item.title.type == .tv ? "Next episode" : "Watch again"))
        } else if let fraction {
            let remaining = ceil(item.duration * (1 - fraction) / 60)
            if remaining.isFinite, remaining < Double(Int.max) {
                parts.append("\(max(1, Int(remaining)))m left")
            }
        } else {
            parts.append("In progress")
        }
        return parts.joined(separator: " · ")
    }

    var body: some View {
        HStack(spacing: 4) {
            Button(action: onPlay) {
                HStack(spacing: 12) {
                    thumbnail
                    VStack(alignment: .leading, spacing: 4) {
                        Text(item.title.title)
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(MelloTVTheme.text)
                            .lineLimit(2)
                        Text(detail)
                            .font(.caption)
                            .foregroundStyle(MelloTVTheme.textSecondary)
                            .lineLimit(2)
                        if showsDate, item.updatedAt.isFinite, item.updatedAt > 0,
                            item.updatedAt < 253_402_300_799
                        {
                            Text(
                                Date(timeIntervalSince1970: item.updatedAt),
                                format: .dateTime.month(.abbreviated).day()
                            )
                            .font(.caption2)
                            .foregroundStyle(MelloTVTheme.textSecondary)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxWidth: .infinity, minHeight: 56, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(isPlaybackDisabled)
            .accessibilityLabel("\(item.completed ? "Play" : "Resume") \(item.title.title)")
            .accessibilityValue(isPreparingPlayback ? "Preparing playback" : detail)

            Button(action: onInfo) {
                Image(systemName: "ellipsis")
                    .font(.body.weight(.medium))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .frame(width: 44, height: 44)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Details for \(item.title.title)")
        }
        .padding(12)
        .appleNeumorphic(cornerRadius: 16, elevation: .subtle)
    }

    private var thumbnail: some View {
        AsyncImage(url: item.title.backdropURL ?? item.title.posterURL) { phase in
            if case .success(let image) = phase {
                image.resizable().scaledToFill()
            } else {
                MelloTVTheme.secondaryBackground
            }
        }
        .frame(width: 80, height: 50)
        .clipped()
        .overlay(.black.opacity(0.2))
        .overlay {
            if isPreparingPlayback {
                ProgressView().tint(.white)
            } else {
                Image(systemName: "play.fill")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)
            }
        }
        .overlay(alignment: .bottomLeading) {
            if let fraction {
                Rectangle().fill(.white.opacity(0.3)).frame(height: 3)
                    .overlay(alignment: .leading) {
                        Rectangle().fill(MelloTVTheme.accent)
                            .frame(width: 80 * fraction, height: 3)
                    }
            }
        }
        .clipShape(.rect(cornerRadius: 9))
        .accessibilityHidden(true)
    }
}
