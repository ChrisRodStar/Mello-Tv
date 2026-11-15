import SwiftUI
import MelloKit

/// 10-foot tvOS Catalog View powering Home, Movies, and TV tabs.
/// Features a cinematic showroom Hero Carousel, Continue Watching shelf,
/// Numbered Top 10 rankings, Panoramic 16:9 cinema rows, and curated category shelves.
public struct CatalogView: View {
    public let category: CatalogCategory

    @State private var featuredTitles: [MediaTitle] = []
    @State private var sections: [CatalogSection] = []
    @State private var continueWatchingItems: [WatchProgress] = []
    @State private var isLoading = true
    @State private var errorMessage: String? = nil

    // Detail & Playback Presentation
    @State private var selectedTitle: MediaTitle? = nil
    @State private var playingTitle: MediaTitle? = nil
    @State private var playingSeason: Int? = nil
    @State private var playingEpisode: Int? = nil
    @State private var playingPosition: Double? = nil

    @ObservedObject private var serverAddress = ServerAddress.shared
    @ObservedObject private var profileStore = ProfileStore.shared

    public init(category: CatalogCategory = .home) {
        self.category = category
    }

    public var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(alignment: .leading, spacing: 48) {
                if isLoading && sections.isEmpty && featuredTitles.isEmpty {
                    loadingStage
                } else if let error = errorMessage, sections.isEmpty {
                    errorStage(error: error)
                } else if sections.isEmpty && featuredTitles.isEmpty {
                    emptyStage
                } else {
                    // 1. Featured Showroom Carousel at top
                    if !featuredTitles.isEmpty {
                        FeaturedCarouselView(
                            titles: featuredTitles,
                            onSelect: { title in
                                selectedTitle = title
                            },
                            onPlay: { title in
                                playTitle(title)
                            }
                        )
                    }

                    // 2. Continue Watching Shelf (Home tab only)
                    if category == .home && !continueWatchingItems.isEmpty {
                        ContinueWatchingShelf(
                            items: continueWatchingItems,
                            onPlay: { progress in
                                playWatchProgress(progress)
                            },
                            onInfo: { title in
                                selectedTitle = title
                            }
                        )
                    }

                    // 3. Curated Shelves: Numbered Top 10, Panoramic 16:9, and Standard 2:3 Shelves
                    curatedShelves
                        .padding(.bottom, 140)
                }
            }
        }
        .edgesIgnoringSafeArea(.top)
        .background(MelloTVTheme.background.ignoresSafeArea())
        .task(id: category) {
            await loadContent()
        }
        .onChange(of: profileStore.activeProfileID) { _, _ in
            Task {
                await loadContent()
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: Notification.Name("MelloTVActiveProfileChanged"))) { _ in
            Task {
                await loadContent()
            }
        }
        .fullScreenCover(item: $selectedTitle) { title in
            TitleDetailView(
                title: title,
                profileID: profileStore.activeProfileID,
                onDismiss: { selectedTitle = nil },
                onPlay: {
                    selectedTitle = nil
                    playTitle(title)
                }
            )
        }
        .fullScreenCover(item: $playingTitle) { title in
            TVPlayerView(
                title: title,
                profileID: profileStore.activeProfileID,
                season: playingSeason,
                episode: playingEpisode,
                initialPosition: playingPosition,
                onDismiss: {
                    playingTitle = nil
                    playingSeason = nil
                    playingEpisode = nil
                    playingPosition = nil
                    if category == .home {
                        Task { await loadContinueWatching() }
                    }
                }
            )
        }
    }

    // MARK: - Curated Shelves

    @ViewBuilder
    private var curatedShelves: some View {
        VStack(alignment: .leading, spacing: 44) {
            ForEach(Array(sections.enumerated()), id: \.element.id) { index, section in
                if index == 0 && section.titles.count >= 4 {
                    // First shelf: Numbered Top 10 Ranking
                    NumberedShelf(
                        section: section,
                        eyebrow: category == .tv ? "Top Shows Today" : "Top 10 Today",
                        onSelect: { selectedTitle = $0 }
                    )
                } else if index == 1 || (index > 2 && index % 3 == 1) {
                    // Second shelf or alternating: Panoramic 16:9 Cinema Row
                    PanoramicShelf(
                        section: section,
                        onSelect: { selectedTitle = $0 }
                    )
                } else {
                    // Standard Curated 2:3 Poster Shelf
                    standardShelf(section: section)
                }
            }
        }
    }

    @ViewBuilder
    private func standardShelf(section: CatalogSection) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 4) {
                Text(section.heading)
                    .font(.title2.weight(.bold))
                    .foregroundStyle(MelloTVTheme.text)
                
                if let reason = section.reason {
                    Text(reason == "liked" ? "Based on titles you liked" : (reason == "watched" ? "Based on titles you watched" : reason))
                        .font(.callout)
                        .foregroundStyle(MelloTVTheme.muted)
                }
            }
            .padding(.horizontal, 48)

            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(alignment: .top, spacing: 28) {
                    ForEach(section.titles) { title in
                        PosterCard(title: title) {
                            selectedTitle = title
                        }
                    }
                }
                .padding(.horizontal, 48)
                .padding(.vertical, 24)
            }
        }
    }

    // MARK: - State Placeholders

    private var loadingStage: some View {
        VStack(spacing: 20) {
            ProgressView()
                .tint(MelloTVTheme.accent)
                .scaleEffect(1.4)
            Text("Loading \(category.title)...")
                .font(.headline)
                .foregroundStyle(MelloTVTheme.muted)
        }
        .frame(maxWidth: .infinity, minHeight: 600)
    }

    private func errorStage(error: String) -> some View {
        VStack(spacing: 20) {
            Image(systemName: "exclamationmark.triangle")
                .font(.system(size: 64))
                .foregroundStyle(.yellow)
            Text("Unable to Load Content")
                .font(.title2.weight(.bold))
                .foregroundStyle(MelloTVTheme.text)
            Text(error)
                .font(.body)
                .foregroundStyle(MelloTVTheme.muted)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 640)
            Button("Try Again") {
                Task { await loadContent() }
            }
            .buttonStyle(.borderedProminent)
            .padding(.top, 12)
        }
        .frame(maxWidth: .infinity, minHeight: 600)
    }

    private var emptyStage: some View {
        VStack(spacing: 20) {
            Image(systemName: category == .movies ? "film" : (category == .tv ? "tv" : "sparkles.tv"))
                .font(.system(size: 64))
                .foregroundStyle(MelloTVTheme.muted)
            Text("No Titles Available")
                .font(.title2.weight(.bold))
                .foregroundStyle(MelloTVTheme.text)
            Text("There are no \(category.title.lowercased()) to display right now.")
                .font(.body)
                .foregroundStyle(MelloTVTheme.muted)
        }
        .frame(maxWidth: .infinity, minHeight: 600)
    }

    // MARK: - Playback Triggers

    private func playTitle(_ title: MediaTitle) {
        playingTitle = title
        playingSeason = nil
        playingEpisode = nil
        playingPosition = nil
    }

    private func playWatchProgress(_ progress: WatchProgress) {
        playingTitle = progress.title
        playingSeason = progress.season > 0 ? progress.season : nil
        playingEpisode = progress.episode > 0 ? progress.episode : nil
        playingPosition = progress.position > 0 ? progress.position : nil
    }

    // MARK: - Data Loading

    private func loadContent() async {
        if profileStore.activeProfileID == nil {
            await profileStore.loadProfiles()
        }
        guard let pid = profileStore.activeProfileID, !pid.isEmpty else {
            return
        }

        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            let response: CatalogResponse
            if let filter = category.mediaTypeFilter {
                response = try await MelloTVAPI.shared.recommendationPage(category: filter, profileID: pid)
            } else {
                response = try await MelloTVAPI.shared.home(profileID: pid)
            }
            var loadedSections = response.sections ?? []

            // Apply category media type filter to sections if not already filtered
            if let filter = category.mediaTypeFilter {
                loadedSections = loadedSections.compactMap { sec in
                    let filtered = sec.titles.filter { $0.type == filter }
                    return filtered.isEmpty ? nil : CatalogSection(id: sec.id, heading: sec.heading, titles: filtered, reason: sec.reason)
                }
            }

            var featured = response.featured ?? []
            if let filter = category.mediaTypeFilter {
                featured = featured.filter { $0.type == filter }
            }

            // Fallback: if featured is empty, pick leading titles from first populated section or response.titles
            if featured.isEmpty {
                if let firstShelf = loadedSections.first(where: { !$0.titles.isEmpty }) {
                    featured = Array(firstShelf.titles.prefix(6))
                } else {
                    featured = Array(response.titles.prefix(6))
                }
            }

            self.sections = loadedSections
            self.featuredTitles = featured
        } catch {
            if sections.isEmpty {
                self.errorMessage = error.localizedDescription
            }
        }

        if category == .home {
            await loadContinueWatching(profileID: pid)
        }
    }

    private func loadContinueWatching(profileID: String? = nil) async {
        guard let pid = profileID ?? profileStore.activeProfileID, !pid.isEmpty else { return }
        do {
            let inProgress = try await MelloTVAPI.shared.fetchContinueWatching(profileID: pid)
            self.continueWatchingItems = inProgress
        } catch {
            if let lib = try? await MelloTVAPI.shared.fetchLibrary(profileId: pid) {
                self.continueWatchingItems = lib.progress.filter { !$0.completed }
            }
        }
    }
}
