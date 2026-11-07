// xcode: set sdk=tvOS

import SwiftUI
import MelloKit

/// 10-foot living room Home feed with Hero billboard and category shelves.
public struct HomeView: View {
    @State private var catalog: [MediaTitle] = []
    @State private var isLoading = true
    @State private var selectedTitle: MediaTitle?
    @State private var playingTitle: MediaTitle?

    public init() {}

    private var featuredTitle: MediaTitle? {
        catalog.first
    }

    private var movies: [MediaTitle] {
        catalog.filter { $0.type == .movie }
    }

    private var tvSeries: [MediaTitle] {
        catalog.filter { $0.type == .tv }
    }

    private var topRated: [MediaTitle] {
        catalog.filter { $0.rating >= 7.5 }
    }

    public var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(alignment: .leading, spacing: 40) {
                // Featured Hero Billboard
                if let featured = featuredTitle {
                    HeroBillboardView(
                        title: featured,
                        onPlay: { playingTitle = featured },
                        onDetails: { selectedTitle = featured }
                    )
                }

                // Shelves
                VStack(alignment: .leading, spacing: 32) {
                    if !movies.isEmpty {
                        shelfSection(title: "Movies", items: movies)
                    }

                    if !tvSeries.isEmpty {
                        shelfSection(title: "TV Series", items: tvSeries)
                    }

                    if !topRated.isEmpty {
                        shelfSection(title: "Top Rated", items: topRated)
                    }
                }
                .padding(.horizontal, 64)
                .padding(.bottom, 64)
            }
        }
        .edgesIgnoringSafeArea(.top)
        .task {
            await loadCatalog()
        }
        .fullScreenCover(item: $selectedTitle) { title in
            TitleDetailView(
                title: title,
                onDismiss: { selectedTitle = nil },
                onPlay: {
                    selectedTitle = nil
                    playingTitle = title
                }
            )
        }
        .fullScreenCover(item: $playingTitle) { title in
            TVPlayerView(title: title, onDismiss: { playingTitle = nil })
        }
    }

    @ViewBuilder
    private func shelfSection(title: String, items: [MediaTitle]) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(title)
                .font(.title2.weight(.bold))
                .foregroundColor(.white)
                .padding(.leading, 8)

            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(spacing: 36) {
                    ForEach(items) { item in
                        PosterCard(title: item) {
                            selectedTitle = item
                        }
                    }
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 24) // Extra clearance for focus scale up
            }
        }
    }

    private func loadCatalog() async {
        isLoading = true
        defer { isLoading = false }
        do {
            catalog = try await MelloTVAPI.shared.fetchCatalog()
        } catch {
            print("Failed to load catalog: \(error)")
        }
    }
}
