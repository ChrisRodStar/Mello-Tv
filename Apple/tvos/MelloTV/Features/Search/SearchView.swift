import SwiftUI
import MelloKit

/// 10-foot focusable Search view on tvOS with Siri Remote voice dictation,
/// quick suggestions, discovery recommendations, and grid of search results.
public struct SearchView: View {
    @State private var query = ""
    @State private var results: [MediaTitle] = []
    @State private var discoveryTitles: [MediaTitle] = []
    @State private var isSearching = false
    @State private var errorMessage: String? = nil
    @State private var selectedTitle: MediaTitle?
    @State private var playingTitle: MediaTitle?

    private let suggestions = [
        "Trending", "Action", "Comedy", "Sci-Fi", "Drama",
        "Animation", "Thriller", "Horror", "Documentary"
    ]

    public init() {}

    private var activeQuery: String {
        query.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    public var body: some View {
        NavigationStack {
            ScrollView(.vertical, showsIndicators: false) {
                VStack(alignment: .leading, spacing: 32) {
                    // Suggestions Bar
                    suggestionsBar
                        .padding(.top, 16)

                    // Body: Loading, No Results, Results Grid, or Discovery Grid
                    if isSearching && results.isEmpty {
                        loadingView
                    } else if !activeQuery.isEmpty && results.isEmpty && !isSearching {
                        noResultsView
                    } else if !results.isEmpty {
                        resultsView
                    } else {
                        discoveryView
                    }
                }
                .padding(.bottom, 64)
            }
            .searchable(text: $query, prompt: "Search movies & TV series")
            .searchSuggestions {
                ForEach(suggestions, id: \.self) { suggestion in
                    Text(suggestion).searchCompletion(suggestion)
                }
            }
            .task(id: query) {
                let current = activeQuery
                guard !current.isEmpty else {
                    results = []
                    isSearching = false
                    return
                }
                do {
                    try await Task.sleep(for: .milliseconds(300))
                    try Task.checkCancellation()
                    await performSearch(query: current)
                } catch {
                    // Cancelled
                }
            }
            .task {
                await loadDiscoveryTitles()
            }
            .background(MelloTVTheme.background.ignoresSafeArea())
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

    // MARK: - Suggestions Row

    private var suggestionsBar: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Suggestions")
                .font(.headline.weight(.semibold))
                .foregroundStyle(MelloTVTheme.muted)
                .padding(.horizontal, 64)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 16) {
                    ForEach(suggestions, id: \.self) { tag in
                        Button(action: {
                            query = tag
                        }) {
                            Text(tag)
                                .font(.callout.weight(.medium))
                                .padding(.horizontal, 20)
                                .padding(.vertical, 10)
                        }
                        .buttonStyle(.bordered)
                        .clipShape(Capsule())
                    }
                }
                .padding(.horizontal, 64)
                .padding(.vertical, 8)
            }
        }
    }

    // MARK: - Results Grid

    private var resultsView: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack {
                Text("\(results.count) \(results.count == 1 ? "result" : "results") for \"\(activeQuery)\"")
                    .font(.title3.weight(.bold))
                    .foregroundStyle(MelloTVTheme.text)
                Spacer()
            }
            .padding(.horizontal, 64)

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
        }
    }

    // MARK: - Discovery View (Idle)

    private var discoveryView: some View {
        VStack(alignment: .leading, spacing: 20) {
            if !discoveryTitles.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Explore Your Catalog")
                        .font(.title2.weight(.bold))
                        .foregroundStyle(MelloTVTheme.text)
                    Text("Find a favorite or discover something new")
                        .font(.callout)
                        .foregroundStyle(MelloTVTheme.muted)
                }
                .padding(.horizontal, 64)

                LazyVGrid(columns: [
                    GridItem(.adaptive(minimum: 220, maximum: 260), spacing: 36)
                ], spacing: 36) {
                    ForEach(discoveryTitles) { item in
                        PosterCard(title: item) {
                            selectedTitle = item
                        }
                    }
                }
                .padding(.horizontal, 64)
            }
        }
    }

    // MARK: - Status Views

    private var loadingView: some View {
        VStack(spacing: 16) {
            ProgressView()
                .scaleEffect(1.4)
                .tint(MelloTVTheme.accent)
            Text("Searching for \"\(activeQuery)\"...")
                .font(.headline)
                .foregroundStyle(MelloTVTheme.muted)
        }
        .frame(maxWidth: .infinity, minHeight: 300)
        .padding(.top, 40)
    }

    private var noResultsView: some View {
        VStack(spacing: 16) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 64))
                .foregroundColor(MelloTVTheme.muted)
            Text("No Results Found")
                .font(.title2.weight(.bold))
                .foregroundColor(MelloTVTheme.text)
            Text("We couldn't find anything matching \"\(activeQuery)\".")
                .font(.body)
                .foregroundColor(MelloTVTheme.muted)
            Text("Try searching for another movie, TV show, or genre.")
                .font(.callout)
                .foregroundColor(MelloTVTheme.muted.opacity(0.8))
        }
        .frame(maxWidth: .infinity, minHeight: 300)
        .padding(.top, 60)
    }

    // MARK: - Data Operations

    private func performSearch(query: String) async {
        isSearching = true
        defer { isSearching = false }

        do {
            let found = try await MelloTVAPI.shared.search(query: query)
            guard !Task.isCancelled else { return }
            self.results = found
        } catch {
            guard !Task.isCancelled else { return }
            self.results = []
        }
    }

    private func loadDiscoveryTitles() async {
        guard discoveryTitles.isEmpty else { return }
        do {
            let catResponse = try await MelloTVAPI.shared.home()
            var seen = Set<String>()
            let titles = catResponse.titles.filter { seen.insert($0.id).inserted }
            self.discoveryTitles = Array(titles.prefix(24))
        } catch {
            // Keep empty on failure
        }
    }
}
