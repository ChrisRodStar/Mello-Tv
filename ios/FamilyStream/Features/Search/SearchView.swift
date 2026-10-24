import Observation
import SwiftUI

struct SearchShelf: Identifiable, Sendable {
    let id: String
    let heading: String
    let titles: [MediaTitle]
}

struct SearchView: View {
    @State private var query = ""
    @State private var store: SearchStore
    @State private var selectedTitle: MediaTitle?
    @State private var playbackRequest: PlaybackRequest?
    @FocusState private var isFieldFocused: Bool
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @Environment(\.dismiss) private var dismiss

    let api: FamilyStreamAPI
    let profileID: String?
    var libraryStore: LibraryStore

    init(api: FamilyStreamAPI, profileID: String?, libraryStore: LibraryStore) {
        self.api = api
        self.profileID = profileID
        self.libraryStore = libraryStore
        _store = State(initialValue: SearchStore(api: api, profileID: profileID))
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                HStack {
                    Text("Search")
                        .font(.title2.weight(.bold))
                        .foregroundStyle(FamilyStreamTheme.text)
                        .frame(maxWidth: .infinity)
                }
                .frame(height: 48)
                .overlay(alignment: .trailing) {
                    Button { dismiss() } label: {
                        Image(systemName: "xmark")
                            .font(.system(size: 14, weight: .semibold))
                            .frame(width: 44, height: 44)
                            .searchGlass()
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(FamilyStreamTheme.textSecondary)
                    .accessibilityLabel("Close search")
                }
                .padding(.horizontal, 20)

                searchHeaderBar

                searchBody
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            .padding(.top, 16)
            .toolbar(.hidden, for: .navigationBar)
            .task { await store.loadDiscovery() }
            .task(id: query) {
                let term = query.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !term.isEmpty else {
                    store.clear()
                    return
                }
                do {
                    try await Task.sleep(for: .milliseconds(300))
                } catch {
                    return
                }
                await store.search(term)
            }
            .sheet(item: $selectedTitle) { title in
                TitleDetailSheet(
                    title: title,
                    profileID: profileID ?? "",
                    libraryStore: libraryStore,
                    onPlay: { request in
                        self.playbackRequest = request
                    }
                )
            }
            .fullScreenCover(item: $playbackRequest) { req in
                PlayerView(
                    request: req,
                    onDismiss: {
                        self.playbackRequest = nil
                        if let profileID {
                            Task {
                                await libraryStore.refresh(for: profileID)
                            }
                        }
                    }
                )
            }
        }
        .background { CatalogPageSurface().ignoresSafeArea() }
        .presentationBackground(.clear)
    }

    // MARK: - Search Header

    private var searchHeaderBar: some View {
        HStack(spacing: 10) {
            HStack(spacing: 8) {
                PhosphorIcon("magnifying-glass", size: 16)
                    .foregroundStyle(FamilyStreamTheme.muted)

                TextField("Movies, shows, and more", text: $query)
                    .font(.subheadline)
                    .foregroundStyle(FamilyStreamTheme.text)
                    .tint(FamilyStreamTheme.accent)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.webSearch)
                    .submitLabel(.search)
                    .focused($isFieldFocused)
                    .onSubmit {
                        isFieldFocused = false
                    }

                if !query.isEmpty {
                    Button {
                        query = ""
                        store.clear()
                    } label: {
                        PhosphorIcon("x-circle", size: 16, weight: .fill)
                            .foregroundStyle(FamilyStreamTheme.muted)
                            .touchTarget(minWidth: 32, minHeight: 32)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Clear search text")
                }
            }
            .padding(.horizontal, 16)
            .frame(height: 52)
            .searchGlass()

            if isFieldFocused || !query.isEmpty {
                Button("Cancel") {
                    withAnimation(.easeInOut(duration: 0.2)) {
                        isFieldFocused = false
                        query = ""
                        store.clear()
                    }
                }
                .font(.subheadline)
                .foregroundStyle(FamilyStreamTheme.textSecondary)
                .touchTarget(minWidth: 44, minHeight: 44)
                .transition(.move(edge: .trailing).combined(with: .opacity))
            }
        }
        .padding(.horizontal, 20)
        .padding(.top, 12)
        .padding(.bottom, 20)
        .animation(.easeInOut(duration: 0.2), value: isFieldFocused || !query.isEmpty)
    }

    // MARK: - Search Body

    @ViewBuilder
    private var searchBody: some View {
        let cleanQuery = query.trimmingCharacters(in: .whitespacesAndNewlines)

        if cleanQuery.isEmpty {
            idleView
        } else if store.isLoading && store.shelves.isEmpty {
            loadingView
        } else if let message = store.errorMessage, store.shelves.isEmpty {
            errorView(message: message, query: cleanQuery)
        } else if store.hasSearched && store.shelves.isEmpty {
            noResultsView(query: cleanQuery)
        } else {
            shelvesView
        }
    }

    @ViewBuilder
    private var idleView: some View {
        if store.discoveryTitles.isEmpty {
            VStack(spacing: 10) {
                Spacer()
                Text("What are you in the mood for?")
                    .font(.title3.weight(.semibold))
                    .foregroundStyle(FamilyStreamTheme.text)
                Text("Find a favorite. Discover your next one.")
                    .font(.subheadline)
                    .foregroundStyle(FamilyStreamTheme.textSecondary)
                Spacer()
            }
            .frame(maxWidth: .infinity)
            .padding(.horizontal, 24)
        } else {
            posterGrid(store.discoveryTitles, heading: "Explore your catalog", paginates: false)
        }
    }

    private var loadingView: some View {
        VStack(spacing: 12) {
            Spacer()

            ProgressView()
                .tint(FamilyStreamTheme.accent)
            Text("Searching...")
                .font(.caption)
                .foregroundStyle(FamilyStreamTheme.mutedSubtle)

            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func noResultsView(query: String) -> some View {
        VStack(spacing: 10) {
            Spacer()

            PhosphorIcon("magnifying-glass", size: 36)
                .foregroundStyle(FamilyStreamTheme.muted.opacity(0.4))
                .padding(.bottom, 2)

            Text("No titles found")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(FamilyStreamTheme.text)

            Text("No matches for \"\(query)\". Check the spelling or try another title.")
                .font(.caption)
                .foregroundStyle(FamilyStreamTheme.muted)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 36)

            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func errorView(message: String, query: String) -> some View {
        VStack(spacing: 12) {
            Spacer()

            PhosphorIcon("wifi-slash", size: 36)
                .foregroundStyle(FamilyStreamTheme.muted.opacity(0.5))
                .padding(.bottom, 2)

            Text("Search unavailable")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(FamilyStreamTheme.text)

            Text(message)
                .font(.caption)
                .foregroundStyle(FamilyStreamTheme.muted)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 36)

            Button {
                Task { await store.search(query) }
            } label: {
                Text("Try again")
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(FamilyStreamTheme.text)
                    .padding(.horizontal, 20)
                    .padding(.vertical, 9)
                    .background(FamilyStreamTheme.cardBackground, in: RoundedRectangle(cornerRadius: 8))
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .stroke(FamilyStreamTheme.border, lineWidth: 0.5)
                    )
                    .touchTarget()
            }
            .buttonStyle(CardScaleButtonStyle(scale: 0.95))
            .padding(.top, 4)

            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var shelvesView: some View {
        posterGrid(store.titles, heading: "Results", paginates: true)
    }

    private func posterGrid(_ titles: [MediaTitle], heading: String, paginates: Bool) -> some View {
        GeometryReader { geometry in
            let gap: CGFloat = 12
            let available = max(1, geometry.size.width - 40)
            let minimum: CGFloat = dynamicTypeSize.isAccessibilitySize ? 160 : 108
            let count = max(1, min(6, Int((available + gap) / (minimum + gap))))
            let width = max(1, (available - CGFloat(count - 1) * gap) / CGFloat(count))
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    HStack(spacing: 8) {
                        Text(heading)
                            .font(.subheadline.weight(.medium))
                            .foregroundStyle(FamilyStreamTheme.textSecondary)
                        if paginates && store.isLoading {
                            ProgressView().controlSize(.small)
                        }
                    }
                    LazyVGrid(columns: Array(repeating: GridItem(.fixed(width), spacing: gap), count: count), spacing: 16) {
                        ForEach(titles) { title in
                            SearchPosterCard(title: title, width: width) {
                                isFieldFocused = false
                                selectedTitle = title
                            }
                            .onAppear {
                                if paginates && title.id == titles.last?.id {
                                    Task { await store.loadMore() }
                                }
                            }
                        }
                    }
                    if paginates && store.isLoadingMore {
                        ProgressView()
                            .tint(FamilyStreamTheme.accent)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 12)
                    }
                }
                .padding(.horizontal, 20)
                .padding(.bottom, 24)
            }
            .scrollDismissesKeyboard(.interactively)
            .scrollIndicators(.hidden)
        }
    }

}

// MARK: - Search Poster Card (Pure Artwork, No Under-Card Labels)

private struct SearchPosterCard: View {
    let title: MediaTitle
    var width: CGFloat? = nil
    let action: () -> Void

    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    private var cardWidth: CGFloat {
        width ?? (dynamicTypeSize.isAccessibilitySize ? 148 : 108)
    }

    private var cardHeight: CGFloat {
        cardWidth * 1.5 // 2:3 aspect ratio
    }

    var body: some View {
        Button(action: action) {
            posterArtwork
                .frame(width: cardWidth, height: cardHeight)
                .artworkCardSurface()
                .contentShape(Rectangle())
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.96))
        .accessibilityLabel(accessibilityLabel)
        .accessibilityHint("Shows title details")
    }

    private var accessibilityLabel: String {
        var parts = [title.title, title.type == .movie ? "Movie" : "Series"]
        if !title.year.isEmpty {
            parts.append(title.year)
        }
        return parts.joined(separator: ", ")
    }

    private var posterArtwork: some View {
        ZStack {
            if let posterURL = title.posterURL {
                AsyncImage(url: posterURL) { phase in
                    switch phase {
                    case .success(let image):
                        image
                            .resizable()
                            .scaledToFill()
                    case .failure:
                        placeholder
                    case .empty:
                        FamilyStreamTheme.cardBackground
                    @unknown default:
                        placeholder
                    }
                }
            } else {
                placeholder
            }
        }
    }

    private var placeholder: some View {
        ZStack {
            FamilyStreamTheme.cardBackground
            VStack(spacing: 6) {
                PhosphorIcon(title.type == .movie ? "film-slate" : "television-simple", size: 24)
                    .foregroundStyle(FamilyStreamTheme.muted.opacity(0.5))
                Text(title.title)
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(FamilyStreamTheme.muted)
                    .multilineTextAlignment(.center)
                    .lineLimit(3)
                    .padding(.horizontal, 6)
            }
        }
    }
}

// MARK: - Search Store

@MainActor
@Observable
final class SearchStore {
    private(set) var discoveryTitles: [MediaTitle] = []
    private(set) var shelves: [SearchShelf] = []
    private(set) var titles: [MediaTitle] = []
    private(set) var isLoading = false
    private(set) var isLoadingMore = false
    private(set) var hasSearched = false
    private(set) var errorMessage: String?
    private(set) var currentPage = 1
    private(set) var totalPages = 1

    var hasMore: Bool {
        currentPage < totalPages
    }

    private let api: FamilyStreamAPI
    private let profileID: String?
    private var activeQuery = ""
    private var activeRequestID = UUID()
    private var cachedCatalogSections: [CatalogSection] = []

    init(api: FamilyStreamAPI, profileID: String?) {
        self.api = api
        self.profileID = profileID
    }

    func loadDiscovery() async {
        guard discoveryTitles.isEmpty else { return }
        do {
            let sections = try await loadCatalogSections()
            guard !Task.isCancelled else { return }
            cachedCatalogSections = sections
            var seen = Set<String>()
            discoveryTitles = Array(sections.flatMap(\.titles).filter {
                $0.posterURL != nil && seen.insert($0.id).inserted
            }.prefix(24))
        } catch {
            // Search remains available when the discovery catalog cannot load.
        }
    }

    func clear() {
        activeRequestID = UUID()
        activeQuery = ""
        shelves = []
        titles = []
        errorMessage = nil
        isLoading = false
        isLoadingMore = false
        hasSearched = false
        currentPage = 1
        totalPages = 1
    }

    func search(_ query: String) async {
        let requestID = UUID()
        activeRequestID = requestID
        activeQuery = query
        isLoading = true
        errorMessage = nil
        currentPage = 1

        do {
            async let searchTask = api.search(query, page: 1)
            async let catalogTask = loadCatalogSections()

            let (searchResponse, catalogSections) = try await (searchTask, catalogTask)
            guard activeRequestID == requestID, !Task.isCancelled else { return }

            let rankedTitles = SearchRanker.rank(titles: searchResponse.titles, for: query)
            self.titles = rankedTitles
            self.currentPage = searchResponse.page
            self.totalPages = searchResponse.totalPages
            self.cachedCatalogSections = catalogSections
            self.shelves = assembleShelves(for: query, searchTitles: rankedTitles, catalogSections: catalogSections)
            self.hasSearched = true
            self.isLoading = false
        } catch {
            guard activeRequestID == requestID, !Task.isCancelled else { return }
            self.errorMessage = error.localizedDescription
            self.hasSearched = true
            self.isLoading = false
        }
    }

    func loadMore() async {
        guard !isLoadingMore, hasMore, !activeQuery.isEmpty else { return }
        let requestID = activeRequestID
        let query = activeQuery
        isLoadingMore = true
        let nextPage = currentPage + 1

        do {
            let response = try await api.search(query, page: nextPage)
            guard activeRequestID == requestID, activeQuery == query, !Task.isCancelled else { return }

            var existingIDs = Set(titles.map(\.id))
            let newTitles = response.titles.filter { existingIDs.insert($0.id).inserted }

            let combined = self.titles + newTitles
            let rankedTitles = SearchRanker.rank(titles: combined, for: query)
            self.titles = rankedTitles
            self.currentPage = response.page
            self.totalPages = response.totalPages
            self.shelves = assembleShelves(for: query, searchTitles: rankedTitles, catalogSections: self.cachedCatalogSections)
            self.isLoadingMore = false
        } catch {
            guard activeRequestID == requestID, activeQuery == query, !Task.isCancelled else { return }
            self.isLoadingMore = false
        }
    }

    private func loadCatalogSections() async throws -> [CatalogSection] {
        if !cachedCatalogSections.isEmpty {
            return cachedCatalogSections
        }
        let homeResponse = try await api.home(profileID: profileID)
        return homeResponse.sections ?? []
    }

    private func assembleShelves(for query: String, searchTitles: [MediaTitle], catalogSections: [CatalogSection]) -> [SearchShelf] {
        guard !searchTitles.isEmpty else { return [] }

        var result: [SearchShelf] = []
        let queryWords = Set(query.lowercased().split(separator: " ").map(String.init))

        // 1. Top Results (The direct matches)
        result.append(SearchShelf(id: "top-results", heading: "Top Results", titles: searchTitles))

        // 2. Type Breakdowns if both Movies and TV exist in results (e.g. Spider-Man has both)
        let movies = searchTitles.filter { $0.type == .movie }
        let tvShows = searchTitles.filter { $0.type == .tv }

        let formattedQuery = query.capitalized
        if movies.count >= 2 && tvShows.count >= 2 {
            result.append(SearchShelf(id: "movies", heading: "\(formattedQuery) Movies", titles: movies))
            result.append(SearchShelf(id: "tv-shows", heading: "\(formattedQuery) TV Shows", titles: tvShows))
        }

        // 3. Collect prominent genres from the search titles
        var genreCounts: [String: Int] = [:]
        for title in searchTitles {
            for genre in title.genres {
                genreCounts[genre, default: 0] += 1
            }
        }
        let prominentGenres = genreCounts.sorted { $0.value > $1.value }.map(\.key)

        // 4. Match & Rank Catalog Shelves
        // Match catalog shelves that correspond to the query or the genres of the results
        var rankedShelves: [(section: CatalogSection, score: Int)] = []
        var seenHeadings = Set<String>()

        for section in catalogSections {
            let headingLower = section.heading.lowercased()
            guard seenHeadings.insert(headingLower).inserted else { continue }
            guard !section.titles.isEmpty else { continue }

            var score = 0

            // Direct query match in shelf heading
            for word in queryWords where headingLower.contains(word) {
                score += 100
            }

            // Genre match from search results
            for genre in prominentGenres {
                let genreLower = genre.lowercased()
                if headingLower.contains(genreLower) {
                    score += 50
                }
            }

            // General high-interest shelves
            if headingLower.contains("popular") || headingLower.contains("trending") || headingLower.contains("top rated") {
                score += 20
            }

            if score > 0 {
                rankedShelves.append((section, score))
            }
        }

        rankedShelves.sort { $0.score > $1.score }

        // Take top 4 most relevant catalog shelves
        for item in rankedShelves.prefix(4) {
            result.append(SearchShelf(id: "catalog-\(item.section.id)", heading: item.section.heading, titles: item.section.titles))
        }

        return result
    }
}

// MARK: - Search Ranker (Multi-Layer Relevance Model)

enum SearchRanker {
    static func rank(titles: [MediaTitle], for query: String) -> [MediaTitle] {
        let cleanQuery = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !cleanQuery.isEmpty else { return titles }

        let normalizedQuery = normalize(cleanQuery)
        let queryTokens = normalizedQuery.split(separator: " ").map(String.init)
        guard !normalizedQuery.isEmpty else { return titles }

        return titles.map { title -> (title: MediaTitle, score: Double) in
            let score = computeScore(
                title: title,
                rawQuery: cleanQuery,
                normalizedQuery: normalizedQuery,
                queryTokens: queryTokens
            )
            return (title, score)
        }
        .sorted { lhs, rhs in
            if abs(lhs.score - rhs.score) > 0.001 {
                return lhs.score > rhs.score
            }
            if lhs.title.rating != rhs.title.rating {
                return lhs.title.rating > rhs.title.rating
            }
            return lhs.title.year > rhs.title.year
        }
        .map(\.title)
    }

    private static func computeScore(
        title: MediaTitle,
        rawQuery: String,
        normalizedQuery: String,
        queryTokens: [String]
    ) -> Double {
        let titleLower = title.title.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let normalizedTitle = normalize(titleLower)

        var score: Double = 0.0

        let strippedTitle = stripLeadingArticle(normalizedTitle)
        let strippedQuery = stripLeadingArticle(normalizedQuery)

        // 1. Exact Match on Normalized Title (+10,000)
        if normalizedTitle == normalizedQuery {
            score += 10_000
        } else if strippedTitle == strippedQuery {
            score += 9_500
        } else if normalizedTitle.hasPrefix(normalizedQuery) {
            // 2. Prefix Match (+4,000)
            score += 4_000
        } else if strippedTitle.hasPrefix(strippedQuery) {
            score += 3_500
        } else if normalizedTitle.contains(normalizedQuery) {
            score += 1_500
        }

        // 3. Multi-token Coverage
        let titleTokens = Set(normalizedTitle.split(separator: " ").map(String.init))
        var matchingTokensCount = 0
        for token in queryTokens {
            if titleTokens.contains(token) || normalizedTitle.contains(token) {
                matchingTokensCount += 1
            }
        }
        if !queryTokens.isEmpty {
            let tokenRatio = Double(matchingTokensCount) / Double(queryTokens.count)
            score += tokenRatio * 1_000
        }

        // 4. Title Coverage Ratio (BM25 Length Normalization)
        // High percentage match beats incidental subtitle match
        if !normalizedTitle.isEmpty {
            let coverage = min(1.0, Double(normalizedQuery.count) / Double(normalizedTitle.count))
            score += coverage * 2_000
        }

        // 5. Quality & Popularity Prior (Rating boost: rating * 50)
        if title.rating > 0 {
            score += title.rating * 50
        }

        // 6. Artwork Availability Boost
        if title.posterURL != nil {
            score += 100
        }

        return score
    }

    private static func normalize(_ text: String) -> String {
        text.folding(options: [.diacriticInsensitive, .caseInsensitive], locale: .current)
            .replacingOccurrences(of: "[-_:—–~]", with: " ", options: .regularExpression)
            .replacingOccurrences(of: "[^a-z0-9 ]", with: "", options: .regularExpression)
            .split(separator: " ")
            .joined(separator: " ")
    }

    private static func stripLeadingArticle(_ text: String) -> String {
        let prefixes = ["the ", "a ", "an "]
        for prefix in prefixes where text.hasPrefix(prefix) {
            return String(text.dropFirst(prefix.count))
        }
        return text
    }
}


private extension View {
    @ViewBuilder
    func searchGlass() -> some View {
        if #available(iOS 26.0, *) {
            self.glassEffect(.regular, in: Capsule())
        } else {
            self.background(.regularMaterial, in: Capsule())
        }
    }
}
