import SwiftUI
import MelloKit

/// Cinematic wide hero billboard displayed at the top of the Home tab.
public struct HeroBillboardView: View {
    public let title: MediaTitle
    public let onPlay: () -> Void
    public let onDetails: () -> Void

    public init(title: MediaTitle, onPlay: @escaping () -> Void, onDetails: @escaping () -> Void) {
        self.title = title
        self.onPlay = onPlay
        self.onDetails = onDetails
    }

    public var body: some View {
        ZStack(alignment: .bottomLeading) {
            // Backdrop Image
            AsyncImage(url: title.backdropURL ?? title.posterURL) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                case .empty, .failure:
                    Color(white: 0.12)
                @unknown default:
                    Color(white: 0.12)
                }
            }
            .frame(height: 540)
            .clipped()
            .overlay(
                // Vignette and linear gradient overlays for readability
                LinearGradient(
                    colors: [
                        Color.clear,
                        Color.black.opacity(0.3),
                        Color.black.opacity(0.95)
                    ],
                    startPoint: .top,
                    endPoint: .bottom
                )
            )
            .overlay(
                LinearGradient(
                    colors: [
                        Color.black.opacity(0.85),
                        Color.black.opacity(0.4),
                        Color.clear
                    ],
                    startPoint: .leading,
                    endPoint: .trailing
                )
            )

            // Content Overlay
            VStack(alignment: .leading, spacing: 14) {
                // Genre tags
                if !title.genres.isEmpty {
                    Text(title.genres.prefix(3).joined(separator: " • ").uppercased())
                        .font(.caption2.weight(.bold))
                        .foregroundColor(.secondary)
                        .tracking(1.5)
                }

                // Title
                Text(title.title)
                    .font(.system(size: 48, weight: .heavy, design: .rounded))
                    .foregroundColor(.white)
                    .lineLimit(2)
                    .shadow(color: .black.opacity(0.8), radius: 8, y: 4)

                // Metadata row
                HStack(spacing: 16) {
                    Text(title.year)
                        .font(.subheadline.weight(.semibold))
                        .foregroundColor(.white.opacity(0.8))

                    if title.rating > 0 {
                        HStack(spacing: 4) {
                            Image(systemName: "star.fill")
                                .font(.system(size: 14))
                                .foregroundColor(.yellow)
                            Text(String(format: "%.1f", title.rating))
                                .font(.subheadline.weight(.bold))
                                .foregroundColor(.white)
                        }
                    }

                    Text(title.type == .movie ? "Movie" : "TV Series")
                        .font(.caption.weight(.bold))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .background(Color.white.opacity(0.2))
                        .clipShape(Capsule())
                }

                // Logline
                if !title.overview.isEmpty {
                    Text(title.overview)
                        .font(.body)
                        .foregroundColor(.white.opacity(0.85))
                        .lineLimit(3)
                        .frame(maxWidth: 700, alignment: .leading)
                        .shadow(color: .black.opacity(0.6), radius: 4)
                }

                // Focusable Action Buttons
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

                    Button(action: onDetails) {
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
                .padding(.top, 8)
            }
            .padding(.leading, 64)
            .padding(.bottom, 48)
        }
        .frame(height: 540)
    }
}
