import SwiftUI

struct TitleDetailSheet: View {
    @Environment(\.dismiss) private var dismiss

    let title: MediaTitle
    let profileID: String
    var libraryStore: LibraryStore
    var onPlay: (PlaybackRequest) -> Void

    var body: some View {
        NavigationStack {
            content(for: title, showsBackButton: false)
                .navigationDestination(for: MediaTitle.self) { relatedTitle in
                    content(for: relatedTitle, showsBackButton: true)
                }
        }
        .presentationDetents([.large])
        .presentationDragIndicator(.hidden)
        .presentationBackground(FamilyStreamTheme.background)
    }

    private func content(for title: MediaTitle, showsBackButton: Bool) -> some View {
        TitleDetailContent(title: title, profileID: profileID, libraryStore: libraryStore,
                           onPlay: onPlay, onClose: { dismiss() }, showsBackButton: showsBackButton)
            .toolbar(.hidden, for: .navigationBar)
    }
}

private struct TitleDetailContent: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    let title: MediaTitle
    let profileID: String
    var libraryStore: LibraryStore
    var onPlay: (PlaybackRequest) -> Void
    var onClose: () -> Void
    let showsBackButton: Bool

    @State private var details: TitleDetails?
    @State private var selectedSeason: Int = 1
    @State private var episodes: [Episode]?
    @State private var isLoading = true
    @State private var isLoadingEpisodes = false
    @State private var errorMessage: String?
    @State private var episodeErrorMessage: String?
    @State private var hasRestoredSavedSeason = false
    @State private var selectedTab: DetailTab = .episodes
    @State private var suggestions: CatalogSection?
    @State private var isLoadingSuggestions = true
    @State private var suggestionsError: String?

    private let api = FamilyStreamAPI(baseURL: ServerAddress.baseURL)
    private let preparedPlayback = PreparedPlaybackService.shared

    private var savedProgress: WatchProgress? {
        libraryStore.savedProgress(for: title)
    }

    private var resumeEpisode: Episode? {
        guard let episodes, !episodes.isEmpty else { return nil }
        let targetNumber: Int
        if let saved = savedProgress, saved.season == selectedSeason {
            let watched = isEpisodeWatched(saved.episode)
            targetNumber = watched ? saved.episode + 1 : saved.episode
        } else {
            targetNumber = episodes.first?.number ?? 1
        }
        let candidate = episodes.first(where: { $0.number == targetNumber }) ?? episodes[0]
        guard isEpisodeWatched(candidate.number) else { return candidate }
        return episodes.first(where: { $0.number >= candidate.number && !isEpisodeWatched($0.number) })
            ?? episodes.first(where: { !isEpisodeWatched($0.number) }) ?? candidate
    }

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0, pinnedViews: [.sectionHeaders]) {
                artworkHeader

                VStack(alignment: .leading, spacing: 16) {
                    playbackButton
                    detailStatus
                }
                .padding(.horizontal, 16)
                .padding(.top, 16)
                .padding(.bottom, 8)

                if title.type == .tv {
                    Section {
                        tvTabContent
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 16)
                            .padding(.top, 16)
                            .padding(.bottom, 24)
                    } header: {
                        detailTabs
                    }
                } else {
                    VStack(alignment: .leading, spacing: 16) {
                        aboutContent
                        suggestionsSection
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, 8)
                    .padding(.bottom, 24)
                }
            }
        }
        .background(FamilyStreamTheme.background)
        .foregroundStyle(FamilyStreamTheme.text)
        .tint(FamilyStreamTheme.accent)
        .task {
            if details == nil { await loadDetails() }
            if suggestions == nil, !Task.isCancelled {
                await loadSuggestions()
            }
        }
        .onChange(of: selectedSeason) { _, newSeason in
            Task {
                await loadEpisodes(for: newSeason)
            }
        }
    }

    private var artworkHeader: some View {
        // The text sizes the header, so long titles and Dynamic Type can grow it.
        VStack(alignment: .leading, spacing: 0) {
            headerControls
            Spacer(minLength: verticalSizeClass == .compact ? 40 : 76)
            Text(title.title)
                .font(.title2.bold())
                .foregroundStyle(.white)
                .shadow(color: .black.opacity(0.85), radius: 5, x: 0, y: 2)
                .multilineTextAlignment(.leading)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .padding(.top, 16)
                .padding(.bottom, 20)
                .accessibilityAddTraits(.isHeader)
        }
        .frame(minHeight: verticalSizeClass == .compact ? 160 : 206)
        .background {
            GeometryReader { geometry in
                AsyncImage(url: title.backdropURL ?? title.posterURL) { phase in
                    if case .success(let image) = phase {
                        image.resizable().scaledToFill()
                    } else {
                        FamilyStreamTheme.secondaryBackground
                    }
                }
                .frame(width: geometry.size.width, height: geometry.size.height)
                .clipped()
                .overlay {
                    // Contrast only where image-overlay text and controls need it.
                    LinearGradient(stops: [
                        .init(color: .black.opacity(0.55), location: 0),
                        .init(color: .clear, location: 0.35),
                        .init(color: .black.opacity(0.2), location: 0.55),
                        .init(color: .black.opacity(0.85), location: 1)
                    ], startPoint: .top, endPoint: .bottom)
                }
                .accessibilityHidden(true)
            }
        }
    }

    private var headerControls: some View {
        HStack(spacing: 8) {
            if showsBackButton {
                Button { dismiss() } label: {
                    PhosphorIcon("caret-left", size: 16, weight: .bold)
                        .foregroundStyle(.white)
                        .frame(width: 34, height: 34)
                        .background(.ultraThinMaterial.opacity(0.85), in: Circle())
                        .touchTarget(minWidth: 44, minHeight: 44)
                }
                .accessibilityLabel("Back to previous title")
            }
            Spacer(minLength: 0)
            Button {
                Task { await libraryStore.toggleMyList(profileID: profileID, title: title) }
            } label: {
                PhosphorIcon("bookmark-simple", size: 16, weight: libraryStore.isInMyList(title) ? .fill : .regular)
                    .foregroundStyle(.white)
                    .frame(width: 34, height: 34)
                    .background(.ultraThinMaterial.opacity(0.85), in: Circle())
                    .touchTarget(minWidth: 44, minHeight: 44)
            }
            .accessibilityLabel(libraryStore.isInMyList(title) ? "Remove from My List" : "Add to My List")
            .accessibilityValue(libraryStore.isInMyList(title) ? "Saved" : "Not saved")

            Button(action: onClose) {
                PhosphorIcon("x", size: 16, weight: .bold)
                    .foregroundStyle(.white)
                    .frame(width: 34, height: 34)
                    .background(.ultraThinMaterial.opacity(0.85), in: Circle())
                    .touchTarget(minWidth: 44, minHeight: 44)
            }
            .accessibilityLabel("Close title details")
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 14)
        .padding(.top, 14)
    }

    private var playbackButton: some View {
        Button(action: startPlayback) {
            Label {
                Text(playbackLabel)
            } icon: {
                PhosphorIcon("play", size: 16, weight: .fill)
            }
                .font(.subheadline.weight(.semibold))
                .multilineTextAlignment(.center)
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .frame(maxWidth: .infinity, minHeight: 44)
                .foregroundStyle(Color.dynamic(lightHex: "ffffff", darkHex: "16171a"))
                .background(Color.dynamic(lightHex: "16171a", darkHex: "f5f5f7"), in: RoundedRectangle(cornerRadius: 10))
                .shadow(color: Color.black.opacity(0.12), radius: 4, y: 2)
                .contentShape(Rectangle())
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.98))
        .disabled(title.type == .tv && (isLoading || isLoadingEpisodes || episodes?.isEmpty != false))
        .opacity(title.type == .tv && (isLoading || isLoadingEpisodes || episodes?.isEmpty != false) ? 0.5 : 1)
    }

    private var playButtonTitle: String {
        if let saved = savedProgress {
            if title.type == .tv {
                return isEpisodeWatched(saved.episode, season: saved.season) ? "Next Episode" : "Resume"
            }
            if !saved.completed { return "Resume" }
        }
        return "Play"
    }

    private var playbackLabel: String {
        guard title.type == .tv, let episode = resumeEpisode ?? episodes?.first else { return playButtonTitle }
        return "\(playButtonTitle) S\(selectedSeason) E\(episode.number)"
    }

    @ViewBuilder
    private var detailStatus: some View {
        if let errorMessage {
            retryStatus(errorMessage) { await loadDetails() }
        } else if isLoading {
            ProgressView("Loading details")
                .font(.subheadline)
                .tint(FamilyStreamTheme.accent)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 8)
        }
    }

    private enum DetailTab: String, CaseIterable, Identifiable {
        case episodes = "Episodes"
        case about = "About"
        case related = "More Like This"
        var id: Self { self }
    }

    private var detailTabs: some View {
        ViewThatFits(in: .horizontal) {
            detailTabButtons
                .fixedSize(horizontal: true, vertical: false)
                .frame(maxWidth: .infinity, alignment: .center)
            ScrollView(.horizontal) {
                detailTabButtons
            }
            .scrollIndicators(.hidden)
        }
        .frame(maxWidth: .infinity, alignment: .center)
        .padding(.horizontal, 16)
        .background(FamilyStreamTheme.background)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Title detail sections")
    }

    private var detailTabButtons: some View {
        HStack(spacing: 24) {
            ForEach(DetailTab.allCases) { tab in
                Button {
                    selectedTab = tab
                } label: {
                    Text(tab.rawValue)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(selectedTab == tab ? FamilyStreamTheme.text : FamilyStreamTheme.muted)
                        .fixedSize(horizontal: true, vertical: false)
                        .frame(minHeight: 48)
                        .overlay(alignment: .bottom) {
                            if selectedTab == tab {
                                Rectangle()
                                    .fill(FamilyStreamTheme.accent)
                                    .frame(height: 2)
                            }
                        }
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(selectedTab == tab ? .isSelected : [])
                .accessibilityHint("Shows \(tab.rawValue.lowercased())")
            }
        }
    }

    @ViewBuilder
    private var tvTabContent: some View {
        switch selectedTab {
        case .episodes:
            if let seasons = details?.seasons, !seasons.isEmpty {
                episodesSection(seasons: seasons)
            } else if !isLoading, errorMessage == nil {
                Text("No episodes are available.")
                    .font(.subheadline)
                    .foregroundStyle(FamilyStreamTheme.muted)
            }
        case .about:
            aboutContent
        case .related:
            suggestionsSection
        }
    }

    private var aboutContent: some View {
        VStack(alignment: .leading, spacing: 12) {
            if !title.overview.isEmpty {
                Text(title.overview)
                    .font(.subheadline)
                    .lineSpacing(4)
                    .foregroundStyle(FamilyStreamTheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if !metadata.isEmpty {
                Text(metadata.joined(separator: "  ·  "))
                    .font(.caption)
                    .foregroundStyle(FamilyStreamTheme.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let genres = details?.genres, !genres.isEmpty {
                Text(genres.joined(separator: "  ·  "))
                    .font(.caption)
                    .foregroundStyle(FamilyStreamTheme.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var metadata: [String] {
        var values: [String] = []
        if !title.year.isEmpty { values.append(title.year) }
        if title.rating > 0 {
            values.append("\(title.rating.formatted(.number.precision(.fractionLength(1)))) / 10")
        }
        if let runtime = details?.runtime, runtime > 0 {
            values.append("\(runtime) min\(title.type == .tv ? " / ep" : "")")
        }
        if title.type == .tv, let seasons = details?.seasons {
            let count = seasons.filter { $0.number > 0 }.count
            values.append("\(count) \(count == 1 ? "season" : "seasons")")
        }
        return values
    }

    private var suggestionsSection: some View {
        VStack(alignment: .leading, spacing: 16) {
            if title.type == .movie || (suggestions != nil && suggestions?.heading != "More Like This") {
                Text(suggestions?.heading ?? "More Like This")
                    .font(.title3.bold())
                    .accessibilityAddTraits(.isHeader)
            }

            if isLoadingSuggestions {
                ProgressView(title.type == .movie ? "Finding movies" : "Finding TV shows")
                    .font(.subheadline)
                    .tint(FamilyStreamTheme.accent)
                    .padding(.vertical, 16)
            } else if let suggestionsError {
                retryStatus(suggestionsError) { await loadSuggestions() }
            } else if let titles = suggestions?.titles, !titles.isEmpty {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: dynamicTypeSize.isAccessibilitySize ? 220 : 104), spacing: 12)],
                          alignment: .leading, spacing: 20) {
                    ForEach(titles) { suggestion in
                        NavigationLink(value: suggestion) {
                            VStack(alignment: .leading, spacing: 8) {
                                Color.clear
                                    .aspectRatio(2.0 / 3, contentMode: .fit)
                                    .overlay {
                                        GeometryReader { geometry in
                                            AsyncImage(url: suggestion.posterURL) { phase in
                                                if case .success(let image) = phase {
                                                    image.resizable().scaledToFill()
                                                } else {
                                                    FamilyStreamTheme.secondaryBackground
                                                        .overlay {
                                                            PhosphorIcon(title.type == .movie ? "film-slate" : "television-simple", size: 24)
                                                                .foregroundStyle(FamilyStreamTheme.muted)
                                                        }
                                                }
                                            }
                                            .frame(width: geometry.size.width, height: geometry.size.height)
                                            .clipped()
                                        }
                                    }
                                    .clipShape(.rect(cornerRadius: 8))
                                    .accessibilityHidden(true)
                                Text(suggestion.title)
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(FamilyStreamTheme.text)
                                    .multilineTextAlignment(.leading)
                                    .fixedSize(horizontal: false, vertical: true)
                                if !suggestion.year.isEmpty {
                                    Text(suggestion.year)
                                        .font(.caption2)
                                        .foregroundStyle(FamilyStreamTheme.muted)
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .topLeading)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(CardScaleButtonStyle(scale: 0.98))
                        .accessibilityLabel("\(suggestion.title)\(suggestion.year.isEmpty ? "" : ", \(suggestion.year)")")
                        .accessibilityHint("Shows title details")
                    }
                }
            } else {
                Text(title.type == .movie ? "More movies will appear as the catalog updates." : "More TV shows will appear as the catalog updates.")
                    .font(.subheadline)
                    .foregroundStyle(FamilyStreamTheme.muted)
            }
        }
        .padding(.top, 8)
    }

    private func loadSuggestions() async {
        isLoadingSuggestions = true
        suggestionsError = nil
        defer { isLoadingSuggestions = false }
        do {
            let loaded = try await api.relatedTitles(to: title, genres: details?.genres ?? [], profileID: profileID)
            try Task.checkCancellation()
            suggestions = loaded
        } catch {
            guard !Task.isCancelled else { return }
            suggestionsError = "Couldn’t load suggestions. Try again."
        }
    }

    private func episodesSection(seasons: [SeasonSummary]) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 16) {
                    seasonSelector(seasons)
                    Spacer(minLength: 0)
                    episodeCount
                }
                VStack(alignment: .leading, spacing: 8) {
                    seasonSelector(seasons, wraps: true)
                    episodeCount
                }
            }

            if isLoadingEpisodes {
                ProgressView("Loading episodes")
                    .font(.subheadline)
                    .tint(FamilyStreamTheme.accent)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 24)
            } else if let episodeErrorMessage {
                retryStatus(episodeErrorMessage) { await loadEpisodes(for: selectedSeason) }
            } else if let episodes, !episodes.isEmpty {
                LazyVStack(alignment: .leading, spacing: 24) {
                    ForEach(episodes) { episode in
                        EpisodeRowView(
                            episode: episode,
                            progress: libraryStore.library.progress.first(where: {
                                $0.title.tmdbID == title.tmdbID &&
                                $0.title.type == .tv &&
                                $0.season == selectedSeason &&
                                $0.episode == episode.number
                            }),
                            isWatched: isEpisodeWatched(episode.number),
                            hasManualWatchedMark: libraryStore.hasManualWatchedMark(
                                profileID: profileID, title: title, season: selectedSeason, episode: episode.number),
                            onPlay: { playEpisode(episode) },
                            onRewatch: { playEpisode(episode, startFromBeginning: true) },
                            onSetWatched: { watched in
                                libraryStore.setEpisodeWatched(watched, profileID: profileID, title: title,
                                                               season: selectedSeason, episode: episode.number)
                            }
                        )
                    }
                }
                .id(selectedSeason)
            } else {
                Text("No episodes are available for this season.")
                    .font(.subheadline)
                    .foregroundStyle(FamilyStreamTheme.muted)
                    .padding(.vertical, 16)
            }
        }
    }

    @ViewBuilder
    private var episodeCount: some View {
        if let episodes, !isLoadingEpisodes {
            Text("\(episodes.count) \(episodes.count == 1 ? "episode" : "episodes")")
                .font(.caption)
                .foregroundStyle(FamilyStreamTheme.muted)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func seasonSelector(_ seasons: [SeasonSummary], wraps: Bool = false) -> some View {
        Menu {
            ForEach(seasons) { season in
                Button {
                    selectedSeason = season.number
                } label: {
                    if season.number == selectedSeason {
                        Label(seasonName(season), image: "check-bold")
                    } else {
                        Text(seasonName(season))
                    }
                }
            }
        } label: {
            HStack(spacing: 8) {
                Text(seasons.first(where: { $0.number == selectedSeason }).map(seasonName) ?? "Season \(selectedSeason)")
                    .fixedSize(horizontal: !wraps, vertical: true)
                if seasons.count > 1 {
                    PhosphorIcon("caret-down", size: 12, weight: .bold)
                }
            }
            .font(.subheadline.weight(.semibold))
            .touchTarget()
        }
        .disabled(seasons.count < 2)
        .accessibilityLabel("Choose season, currently season \(selectedSeason)")
    }

    private func seasonName(_ season: SeasonSummary) -> String {
        season.name.isEmpty ? "Season \(season.number)" : season.name
    }

    private func retryStatus(_ message: String, retry: @escaping () async -> Void) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(message)
                .font(.subheadline)
                .foregroundStyle(FamilyStreamTheme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            Button("Try again") { Task { await retry() } }
                .font(.subheadline.weight(.semibold))
                .touchTarget()
        }
        .padding(.vertical, 8)
    }

    // MARK: - Playback Helpers

    private func startPlayback() {
        if title.type == .movie {
            let req = PlaybackRequest(
                titleID: title.tmdbID,
                type: .movie,
                title: title.title,
                profileID: profileID,
                catalogTitle: title
            )
            let timed = req.traced(.play)
            onClose()
            onPlay(timed)
        } else if let episodes, !episodes.isEmpty {
            let targetEpisode = resumeEpisode ?? episodes[0]
            let req = PlaybackRequest(
                titleID: title.tmdbID,
                type: .tv,
                title: title.title,
                profileID: profileID,
                season: selectedSeason,
                episode: targetEpisode.number,
                episodeTitle: targetEpisode.name,
                catalogTitle: title
            )
            let timed = req.traced(.play)
            onClose()
            onPlay(timed)
        }
    }

    private func isEpisodeWatched(_ episode: Int, season: Int? = nil) -> Bool {
        libraryStore.isEpisodeWatched(profileID: profileID, title: title, season: season ?? selectedSeason, episode: episode)
    }

    private func playEpisode(_ episode: Episode, startFromBeginning: Bool = false) {
        let req = PlaybackRequest(
            titleID: title.tmdbID,
            type: .tv,
            title: title.title,
            profileID: profileID,
            season: selectedSeason,
            episode: episode.number,
            episodeTitle: episode.name,
            catalogTitle: title,
            startFromBeginning: startFromBeginning
        )
        let timed = req.traced(startFromBeginning ? .restart : .play)
        onClose()
        onPlay(timed)
    }

    private func preloadEpisode(_ episode: Episode) {
        let req = PlaybackRequest(
            titleID: title.tmdbID,
            type: .tv,
            title: title.title,
            profileID: profileID,
            season: selectedSeason,
            episode: episode.number,
            episodeTitle: episode.name,
            catalogTitle: title
        )
        preparedPlayback.preload(request: req)
    }

    private func loadDetails() async {
        isLoading = true
        errorMessage = nil
        do {
            let loaded = try await api.details(for: title)
            self.details = loaded
            self.isLoading = false

            if title.type == .tv {
                let initialSeason: Int
                if !hasRestoredSavedSeason, let saved = savedProgress {
                    initialSeason = saved.season
                    hasRestoredSavedSeason = true
                } else {
                    initialSeason = loaded.seasons.first(where: { $0.number > 0 })?.number ?? 1
                }
                self.selectedSeason = initialSeason
                await loadEpisodes(for: initialSeason)
            } else {
                let req = PlaybackRequest(
                    titleID: title.tmdbID,
                    type: .movie,
                    title: title.title,
                    profileID: profileID,
                    catalogTitle: title
                )
                preparedPlayback.preload(request: req)
            }
        } catch {
            self.errorMessage = error.localizedDescription
            self.isLoading = false
        }
    }

    private func loadEpisodes(for seasonNumber: Int) async {
        isLoadingEpisodes = true
        episodeErrorMessage = nil
        do {
            let epList = try await api.seasonEpisodes(tvID: title.tmdbID, season: seasonNumber)
            self.episodes = epList
            self.isLoadingEpisodes = false

            // Preload resume episode or first episode only
            if let first = epList.first {
                preloadEpisode(resumeEpisode ?? first)
            }
        } catch {
            self.episodeErrorMessage = error.localizedDescription
            self.isLoadingEpisodes = false
        }
    }
}
