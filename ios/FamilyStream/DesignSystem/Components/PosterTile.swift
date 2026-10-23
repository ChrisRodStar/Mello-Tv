import SwiftUI

struct PosterTile: View {
    let title: MediaTitle
    var action: (() -> Void)? = nil

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
            .frame(width: 126, height: 189)
            .clipShape(RoundedRectangle(cornerRadius: 11, style: .continuous))
            .appleNeumorphic(cornerRadius: 11, elevation: .medium)
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
                    ZStack {
                        FamilyStreamTheme.cardBackground
                        ProgressView()
                            .tint(.white.opacity(0.6))
                            .scaleEffect(0.8)
                    }
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
            FamilyStreamTheme.secondaryBackground

            VStack(spacing: 8) {
                PhosphorIcon(title.type == .movie ? "film-slate" : "television-simple", size: 24)
                    .foregroundStyle(FamilyStreamTheme.muted)

                Text(title.title)
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(.white.opacity(0.8))
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
