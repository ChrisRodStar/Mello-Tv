import SwiftUI
import MelloKit

/// 10-foot focusable poster card for movies and TV series.
public struct PosterCard: View {
    public let title: MediaTitle
    public let action: () -> Void

    public init(title: MediaTitle, action: @escaping () -> Void) {
        self.title = title
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 12) {
                AsyncImage(url: title.posterURL) { phase in
                    switch phase {
                    case .success(let image):
                        image
                            .resizable()
                            .aspectRatio(contentMode: .fill)
                    case .empty, .failure:
                        ZStack {
                            Color(white: 0.15)
                            VStack(spacing: 8) {
                                Image(systemName: title.type == .movie ? "film" : "tv")
                                    .font(.system(size: 40))
                                    .foregroundColor(.secondary)
                                Text(title.title)
                                    .font(.caption)
                                    .foregroundColor(.secondary)
                                    .multilineTextAlignment(.center)
                                    .padding(.horizontal, 8)
                            }
                        }
                    @unknown default:
                        Color(white: 0.15)
                    }
                }
                .frame(width: 220, height: 330)
                .tvCardFocusStyle(cornerRadius: 14)

                VStack(alignment: .leading, spacing: 2) {
                    Text(title.title)
                        .font(.headline)
                        .foregroundColor(.primary)
                        .lineLimit(1)
                    
                    HStack(spacing: 8) {
                        Text(title.year)
                            .font(.caption2)
                            .foregroundColor(.secondary)
                        
                        if title.rating > 0 {
                            HStack(spacing: 3) {
                                Image(systemName: "star.fill")
                                    .font(.system(size: 10))
                                    .foregroundColor(.yellow)
                                Text(String(format: "%.1f", title.rating))
                                    .font(.caption2)
                                    .foregroundColor(.secondary)
                            }
                        }
                    }
                }
                .frame(width: 220, alignment: .leading)
            }
        }
        .buttonStyle(.plain)
    }
}
