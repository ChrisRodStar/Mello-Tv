import SwiftUI
import MelloKit

/// 10-foot Personal Library view showing Watchlist and In-Progress titles.
public struct LibraryView: View {
    @State private var library: ProfileLibrary = .empty
    @State private var isLoading = true
    @State private var selectedTitle: MediaTitle?
    @State private var playingTitle: MediaTitle?

    public init() {}

    public var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(alignment: .leading, spacing: 40) {
                if !library.progress.isEmpty {
                    VStack(alignment: .leading, spacing: 16) {
                        Text("Continue Watching")
                            .font(.title2.weight(.bold))
                            .foregroundColor(.white)

                        ScrollView(.horizontal, showsIndicators: false) {
                            LazyHStack(spacing: 36) {
                                ForEach(library.progress, id: \.title.id) { prog in
                                    PosterCard(title: prog.title) {
                                        selectedTitle = prog.title
                                    }
                                }
                            }
                            .padding(.vertical, 24)
                        }
                    }
                }

                if !library.list.isEmpty {
                    VStack(alignment: .leading, spacing: 16) {
                        Text("My Watchlist")
                            .font(.title2.weight(.bold))
                            .foregroundColor(.white)

                        LazyVGrid(columns: [
                            GridItem(.adaptive(minimum: 220, maximum: 260), spacing: 36)
                        ], spacing: 36) {
                            ForEach(library.list) { item in
                                PosterCard(title: item) {
                                    selectedTitle = item
                                }
                            }
                        }
                        .padding(.vertical, 24)
                    }
                }

                if library.progress.isEmpty && library.list.isEmpty && !isLoading {
                    VStack(spacing: 16) {
                        Image(systemName: "bookmark.slash")
                            .font(.system(size: 56))
                            .foregroundColor(.secondary)
                        Text("Your Library is Empty")
                            .font(.title2.weight(.bold))
                            .foregroundColor(.white)
                        Text("Items you start watching or add to your watchlist will appear here.")
                            .font(.body)
                            .foregroundColor(.secondary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.top, 120)
                }
            }
            .padding(.horizontal, 64)
            .padding(.top, 32)
            .padding(.bottom, 64)
        }
        .task {
            await loadLibrary()
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

    private func loadLibrary() async {
        isLoading = true
        defer { isLoading = false }
        do {
            self.library = try await MelloTVAPI.shared.fetchLibrary(profileId: "default")
        } catch {
            print("Failed to load library: \(error)")
        }
    }
}
