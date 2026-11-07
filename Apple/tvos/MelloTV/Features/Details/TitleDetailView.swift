import SwiftUI
import MelloKit

/// 10-foot Title Details screen with synopsis, metadata, and episode browsing.
public struct TitleDetailView: View {
    public let title: MediaTitle
    public let onDismiss: () -> Void
    public let onPlay: () -> Void

    @State private var selectedSeason: Int = 1
    @State private var episodes: [Episode] = []
    @State private var isLoadingEpisodes = false
    @State private var playingEpisode: Episode?

    public init(title: MediaTitle, onDismiss: @escaping () -> Void, onPlay: @escaping () -> Void) {
        self.title = title
        self.onDismiss = onDismiss
        self.onPlay = onPlay
    }

    public var body: some View {
        ZStack {
            // Ambient Backdrop
            AsyncImage(url: title.backdropURL ?? title.posterURL) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                case .empty, .failure:
                    Color(white: 0.1)
                @unknown default:
                    Color(white: 0.1)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .clipped()
            .overlay(Color.black.opacity(0.85))
            .edgesIgnoringSafeArea(.all)

            ScrollView(.vertical, showsIndicators: false) {
                VStack(alignment: .leading, spacing: 36) {
                    // Header metadata & actions
                    HStack(alignment: .top, spacing: 48) {
                        // Poster
                        AsyncImage(url: title.posterURL) { phase in
                            if let img = phase.image {
                                img.resizable().aspectRatio(contentMode: .fill)
                            } else {
                                Color(white: 0.2)
                            }
                        }
                        .frame(width: 280, height: 420)
                        .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
                        .shadow(color: .black.opacity(0.8), radius: 24, y: 12)

                        // Info & Controls
                        VStack(alignment: .leading, spacing: 18) {
                            Text(title.title)
                                .font(.system(size: 44, weight: .heavy, design: .rounded))
                                .foregroundColor(.white)

                            HStack(spacing: 16) {
                                Text(title.year)
                                    .font(.title3.weight(.medium))
                                    .foregroundColor(.white.opacity(0.8))

                                if title.rating > 0 {
                                    HStack(spacing: 4) {
                                        Image(systemName: "star.fill")
                                            .foregroundColor(.yellow)
                                        Text(String(format: "%.1f", title.rating))
                                            .fontWeight(.bold)
                                    }
                                    .font(.title3)
                                }

                                Text(title.type.displayName)
                                    .font(.callout.weight(.bold))
                                    .padding(.horizontal, 10)
                                    .padding(.vertical, 4)
                                    .background(Color.white.opacity(0.2))
                                    .clipShape(Capsule())
                            }

                            if !title.genres.isEmpty {
                                Text(title.genres.joined(separator: " • "))
                                    .font(.headline)
                                    .foregroundColor(.secondary)
                            }

                            Text(title.overview)
                                .font(.body)
                                .foregroundColor(.white.opacity(0.9))
                                .lineLimit(5)
                                .padding(.top, 4)

                            HStack(spacing: 24) {
                                Button(action: onPlay) {
                                    HStack(spacing: 12) {
                                        Image(systemName: "play.fill")
                                        Text(title.type == .movie ? "Play Movie" : "Play S1 E1")
                                            .fontWeight(.bold)
                                    }
                                    .padding(.horizontal, 28)
                                    .padding(.vertical, 14)
                                }
                                .buttonStyle(.borderedProminent)

                                Button(action: onDismiss) {
                                    Text("Close")
                                        .padding(.horizontal, 24)
                                        .padding(.vertical, 14)
                                }
                                .buttonStyle(.bordered)
                            }
                            .padding(.top, 12)
                        }
                    }
                    .padding(.top, 40)

                    // For TV Series: Episodes Carousel
                    if title.type == .tv {
                        VStack(alignment: .leading, spacing: 20) {
                            Text("Episodes")
                                .font(.title2.weight(.bold))
                                .foregroundColor(.white)

                            if isLoadingEpisodes {
                                ProgressView()
                                    .frame(maxWidth: .infinity, alignment: .center)
                                    .padding()
                            } else if episodes.isEmpty {
                                Text("No episodes available")
                                    .foregroundColor(.secondary)
                            } else {
                                ScrollView(.horizontal, showsIndicators: false) {
                                    LazyHStack(spacing: 32) {
                                        ForEach(episodes) { ep in
                                            episodeCard(ep)
                                        }
                                    }
                                    .padding(.vertical, 20)
                                }
                            }
                        }
                    }
                }
                .padding(.horizontal, 64)
                .padding(.bottom, 64)
            }
        }
        .task {
            if title.type == .tv {
                await loadEpisodes()
            }
        }
        .fullScreenCover(item: $playingEpisode) { ep in
            TVPlayerView(
                title: title,
                season: selectedSeason,
                episode: ep.number,
                onDismiss: { playingEpisode = nil }
            )
        }
    }

    @ViewBuilder
    private func episodeCard(_ ep: Episode) -> some View {
        Button {
            playingEpisode = ep
        } label: {
            VStack(alignment: .leading, spacing: 10) {
                AsyncImage(url: ep.stillURL) { phase in
                    if let img = phase.image {
                        img.resizable().aspectRatio(contentMode: .fill)
                    } else {
                        ZStack {
                            Color(white: 0.18)
                            Image(systemName: "play.rectangle.fill")
                                .font(.system(size: 32))
                                .foregroundColor(.secondary)
                        }
                    }
                }
                .frame(width: 320, height: 180)
                .tvCardFocusStyle(cornerRadius: 12)

                VStack(alignment: .leading, spacing: 2) {
                    Text("Episode \(ep.number)")
                        .font(.caption.weight(.bold))
                        .foregroundColor(.secondary)
                    Text(ep.name)
                        .font(.headline)
                        .foregroundColor(.primary)
                        .lineLimit(1)
                }
                .frame(width: 320, alignment: .leading)
            }
        }
        .buttonStyle(.plain)
    }

    private func loadEpisodes() async {
        isLoadingEpisodes = true
        defer { isLoadingEpisodes = false }
        do {
            let details = try await MelloTVAPI.shared.fetchSeasonDetails(showId: title.tmdbID, seasonNumber: selectedSeason)
            self.episodes = details.episodes
        } catch {
            print("Failed to load season details: \(error)")
        }
    }
}
