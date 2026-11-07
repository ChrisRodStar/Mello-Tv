import SwiftUI
import MelloKit

/// 10-foot focusable Search view with Siri Remote dictation and keyboard support.
public struct SearchView: View {
    @State private var query = ""
    @State private var results: [MediaTitle] = []
    @State private var isSearching = false
    @State private var selectedTitle: MediaTitle?
    @State private var playingTitle: MediaTitle?

    public init() {}

    public var body: some View {
        NavigationStack {
            ScrollView(.vertical, showsIndicators: false) {
                if isSearching {
                    ProgressView()
                        .padding(.top, 80)
                } else if results.isEmpty && !query.isEmpty {
                    VStack(spacing: 16) {
                        Image(systemName: "magnifyingglass")
                            .font(.system(size: 56))
                            .foregroundColor(.secondary)
                        Text("No Results Found")
                            .font(.title2.weight(.bold))
                            .foregroundColor(.white)
                        Text("Try searching for another movie or TV show.")
                            .font(.body)
                            .foregroundColor(.secondary)
                    }
                    .padding(.top, 100)
                } else {
                    LazyVGrid(columns: [
                        GridItem(.adaptive(minimum: 220, maximum: 260), spacing: 36)
                    ], spacing: 36) {
                        ForEach(results) { item in
                            PosterCard(title: item) {
                                selectedTitle = item
                            }
                        }
                    }
                    .padding(.horizontal, 64)
                    .padding(.top, 32)
                    .padding(.bottom, 64)
                }
            }
            .searchable(text: $query, prompt: "Search movies & TV series")
            .onChange(of: query) { _, newQuery in
                Task {
                    await performSearch(query: newQuery)
                }
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
    }

    private func performSearch(query: String) async {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            self.results = []
            return
        }

        isSearching = true
        defer { isSearching = false }

        do {
            self.results = try await MelloTVAPI.shared.search(query: trimmed)
        } catch {
            print("Search failed: \(error)")
        }
    }
}
