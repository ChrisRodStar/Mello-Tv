import SwiftUI
import MelloKit

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
        .presentationBackground(MelloTVTheme.background)
        .background { MelloTVTheme.background.ignoresSafeArea() }
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
    @State private var episodeRequestID = UUID()
    @State private var selectedTab: DetailTab = .episodes
    @State private var suggestions: CatalogSection?
    @State private var isLoadingSuggestions = true
    @State private var suggestionsError: String?

    private let api = MelloTVAPI(baseURL: ServerAddress.baseURL)
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
                .padding(.horizontal, 20)
                .padding(.top, 16)
                .padding(.bottom, 8)

                if title.type == .tv {
                    Section {
                        tvTabContent
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 20)
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
                    .padding(.horizontal, 20)
                    .padding(.top, 8)
                    .padding(.bottom, 24)
                }
            }
        }
        .background { DetailPageSurface(title: title).ignoresSafeArea() }
        .foregroundStyle(MelloTVTheme.text)
        .tint(MelloTVTheme.accent)
        .task {
            if details == nil { await loadDetails() }
            if suggestions == nil, !Task.isCancelled {
                await loadSuggestions()
            }
        }
        .task(id: episodeLoadKey) {
            guard episodeLoadKey != nil else { return }
            await loadEpisodes(for: selectedSeason)
        }
    }

    private var episodeLoadKey: String? {
        guard title.type == .tv, let details,
              details.seasons.contains(where: { $0.number == selectedSeason }) else { return nil }
        return "\(title.id)-\(selectedSeason)"
    }

    private var artworkHeader: some View {
        VStack(alignment: .leading, spacing: 0) {
            headerControls
            Spacer(minLength: verticalSizeClass == .compact ? 130 : 210)
            VStack(alignment: .leading, spacing: 10) {
                Text(title.title)
                    .font(.largeTitle.weight(.bold))
                    .tracking(-0.8)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                let summary = [title.year, title.genres.first ?? ""].filter { !$0.isEmpty }
                if !summary.isEmpty {
                    Text(summary.joined(separator: " · "))
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 20)
            .padding(.top, 32)
            .padding(.bottom, 8)
        }
        .frame(minHeight: verticalSizeClass == .compact ? 300 : 400)
        .background {
            GeometryReader { geometry in
                AsyncImage(url: title.backdropURL ?? title.posterURL) { phase in
                    if case .success(let image) = phase {
                        image.resizable().scaledToFill()
                    } else {
                        LinearGradient(colors: [MelloTVTheme.secondaryBackground, MelloTVTheme.background],
                                       startPoint: .top, endPoint: .bottom)
                    }
                }
                .frame(width: geometry.size.width, height: verticalSizeClass == .compact ? 240 : 340)
                .clipped()
                .layerEffect(ShaderLibrary.detailArtworkBlur(
                    .float2(Float(geometry.size.width), verticalSizeClass == .compact ? 240 : 340)
                ), maxSampleOffset: CGSize(width: 26, height: 26))
                .clipped()
                .mask {
                    LinearGradient(stops: [
                        .init(color: .black.opacity(0.88), location: 0),
                        .init(color: .black, location: 0.52),
                        .init(color: .black.opacity(0.85), location: 0.65),
                        .init(color: .black.opacity(0.30), location: 0.80),
                        .init(color: .clear, location: 0.94),
                        .init(color: .clear, location: 1)
                    ], startPoint: .top, endPoint: .bottom)
                }
                .overlay(alignment: .top) {
                    LinearGradient(colors: [.black.opacity(0.12), .clear], startPoint: .top, endPoint: .bottom)
                        .frame(height: 110)
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
                        .frame(width: 44, height: 44)
                        .detailGlass(in: Circle())
                }
                .accessibilityLabel("Back to previous title")
            }
            Spacer(minLength: 0)
            Button {
                Task { await libraryStore.toggleMyList(profileID: profileID, title: title) }
            } label: {
                PhosphorIcon("bookmark-simple", size: 16, weight: libraryStore.isInMyList(title) ? .fill : .regular)
                    .foregroundStyle(.white)
                    .frame(width: 44, height: 44)
                    .detailGlass(in: Circle())
            }
            .accessibilityLabel(libraryStore.isInMyList(title) ? "Remove from My List" : "Add to My List")
            .accessibilityValue(libraryStore.isInMyList(title) ? "Saved" : "Not saved")

            Button(action: onClose) {
                PhosphorIcon("x", size: 16, weight: .bold)
                    .foregroundStyle(.white)
                    .frame(width: 44, height: 44)
                    .detailGlass(in: Circle())
            }
            .accessibilityLabel("Close title details")
        }
        .buttonStyle(.plain)
        .environment(\.colorScheme, .dark)
        .padding(.horizontal, 20)
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
                .padding(.horizontal, 20)
                .padding(.vertical, 16)
                .frame(maxWidth: .infinity, minHeight: 44)
                .foregroundStyle(Color.dynamic(lightHex: "ffffff", darkHex: "16171a"))
                .background(Color.dynamic(lightHex: "16171a", darkHex: "f5f5f7"), in: Capsule())
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
                guard saved.season == selectedSeason, let episode = resumeEpisode else { return "Play" }
                if episode.number == saved.episode && !isEpisodeWatched(saved.episode) { return "Resume" }
                return episode.number > saved.episode ? "Next Episode" : "Play"
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
                .tint(MelloTVTheme.accent)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 8)
        }
    }

    private enum DetailTab: String, CaseIterable, Identifiable {
        case episodes = "Episodes"
        case about = "About"
        case related = "Related"
        var id: Self { self }
    }

    private var detailTabs: some View {
        Group {
            if dynamicTypeSize.isAccessibilitySize {
                VStack(spacing: 4) {
                    ForEach(DetailTab.allCases) { tab in
                        Button { selectedTab = tab } label: {
                            HStack {
                                Text(tab.rawValue)
                                Spacer()
                                if selectedTab == tab { Image(systemName: "checkmark") }
                            }
                            .padding(12)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityAddTraits(selectedTab == tab ? .isSelected : [])
                    }
                }
            } else {
                Picker("Title detail sections", selection: $selectedTab) {
                    ForEach(DetailTab.allCases) { tab in Text(tab.rawValue).tag(tab) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .padding(4)
                .detailGlass(in: Capsule())
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity)
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
                    .foregroundStyle(MelloTVTheme.muted)
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
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if !metadata.isEmpty {
                Text(metadata.joined(separator: "  ·  "))
                    .font(.caption)
                    .foregroundStyle(MelloTVTheme.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let genres = details?.genres, !genres.isEmpty {
                Text(genres.joined(separator: "  ·  "))
                    .font(.caption)
                    .foregroundStyle(MelloTVTheme.muted)
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
                    .tint(MelloTVTheme.accent)
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
                                                    MelloTVTheme.secondaryBackground
                                                        .overlay {
                                                            PhosphorIcon(title.type == .movie ? "film-slate" : "television-simple", size: 24)
                                                                .foregroundStyle(MelloTVTheme.muted)
                                                        }
                                                }
                                            }
                                            .frame(width: geometry.size.width, height: geometry.size.height)
                                            .clipped()
                                        }
                                    }
                                    .artworkCardSurface()
                                    .accessibilityHidden(true)
                                Text(suggestion.title)
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(MelloTVTheme.text)
                                    .multilineTextAlignment(.leading)
                                    .fixedSize(horizontal: false, vertical: true)
                                if !suggestion.year.isEmpty {
                                    Text(suggestion.year)
                                        .font(.caption2)
                                        .foregroundStyle(MelloTVTheme.muted)
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
                    .foregroundStyle(MelloTVTheme.muted)
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
                    .tint(MelloTVTheme.accent)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 24)
            } else if let episodeErrorMessage {
                retryStatus(episodeErrorMessage) { await loadEpisodes(for: selectedSeason) }
            } else if let episodes, !episodes.isEmpty {
                LazyVStack(alignment: .leading, spacing: 12) {
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
                            isNextUp: resumeEpisode?.number == episode.number && !isEpisodeWatched(episode.number),
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
                    .foregroundStyle(MelloTVTheme.muted)
                    .padding(.vertical, 16)
            }
        }
    }

    @ViewBuilder
    private var episodeCount: some View {
        if let episodes, !isLoadingEpisodes {
            Text("\(episodes.count) \(episodes.count == 1 ? "episode" : "episodes")")
                .font(.caption)
                .foregroundStyle(MelloTVTheme.muted)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func seasonSelector(_ seasons: [SeasonSummary], wraps: Bool = false) -> some View {
        Menu {
            ForEach(seasons) { season in
                Button {
                    guard selectedSeason != season.number else { return }
                    episodes = nil
                    isLoadingEpisodes = true
                    selectedSeason = season.number
                } label: {
                    if season.number == selectedSeason {
                        Label(seasonName(season), systemImage: "checkmark")
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
            .padding(.horizontal, 16)
            .frame(minHeight: 44)
            .detailGlass(in: Capsule())
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
                .foregroundStyle(MelloTVTheme.textSecondary)
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
            try Task.checkCancellation()
            if title.type == .tv {
                let initialSeason: Int
                if !hasRestoredSavedSeason, let saved = savedProgress,
                   loaded.seasons.contains(where: { $0.number == saved.season }) {
                    initialSeason = saved.season
                    hasRestoredSavedSeason = true
                } else {
                    initialSeason = loaded.seasons.first(where: { $0.number > 0 })?.number ?? loaded.seasons.first?.number ?? 1
                }
                self.selectedSeason = initialSeason
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
            self.details = loaded
            self.isLoading = false
        } catch {
            guard !Task.isCancelled else { return }
            self.errorMessage = error.localizedDescription
            self.isLoading = false
        }
    }

    private func loadEpisodes(for seasonNumber: Int) async {
        let requestID = UUID()
        episodeRequestID = requestID
        isLoadingEpisodes = true
        episodes = nil
        episodeErrorMessage = nil
        do {
            let epList = try await api.seasonEpisodes(tvID: title.tmdbID, season: seasonNumber)
            guard !Task.isCancelled, episodeRequestID == requestID, selectedSeason == seasonNumber else { return }
            self.episodes = epList.sorted { $0.number < $1.number }
            self.isLoadingEpisodes = false

            // Preload resume episode or first episode only
            if let first = epList.first {
                preloadEpisode(resumeEpisode ?? first)
            }
        } catch {
            guard !Task.isCancelled, episodeRequestID == requestID, selectedSeason == seasonNumber else { return }
            self.episodeErrorMessage = error.localizedDescription
            self.isLoadingEpisodes = false
        }
    }
}

/// A quiet, artwork-tinted canvas for reading and browsing episodes.
private struct DetailPageSurface: View {
    let title: MediaTitle
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .top) {
                MelloTVTheme.background
                AsyncImage(url: title.backdropURL ?? title.posterURL) { phase in
                    if case .success(let image) = phase {
                        image.resizable()
                            .scaledToFill()
                            .frame(width: geometry.size.width, height: min(640, geometry.size.height))
                            .clipped()
                            .blur(radius: 80, opaque: true)
                            .saturation(0.30)
                            .opacity(colorScheme == .dark ? 0.22 : 0.10)
                            .mask {
                                LinearGradient(stops: [
                                    .init(color: .black, location: 0),
                                    .init(color: .black.opacity(0.65), location: 0.45),
                                    .init(color: .clear, location: 1)
                                ], startPoint: .top, endPoint: .bottom)
                            }
                    }
                }
            }
            .clipped()
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

private extension View {
    @ViewBuilder
    func detailGlass<S: Shape>(in shape: S) -> some View {
        if #available(iOS 26.0, *) {
            self.glassEffect(.regular, in: shape)
        } else {
            self.background(.regularMaterial, in: shape)
        }
    }
}
