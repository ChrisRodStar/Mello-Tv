import SwiftUI
import MelloKit

struct CatalogView: View {
    @State private var store: CatalogStore
    @State private var selectedTitle: MediaTitle?
    @State private var heroScrollOffset: CGFloat = 0
    @State private var foregroundRefreshGeneration = 0
    @State private var completedForegroundRefreshGeneration = 0
    @State private var hasEnteredBackground = false
    @Environment(\.scenePhase) private var scenePhase
    @State private var playbackRequest: PlaybackRequest?
    @State private var resolvingTitleID: String?
    @State private var resumeTask: Task<Void, Never>?
    @State private var preloadTask: Task<Void, Never>?
    @State private var preloadedRequest: (item: WatchProgress, request: PlaybackRequest)?

    let profileID: String?
    var libraryStore: LibraryStore

    init(api: MelloTVAPI, category: CatalogCategory, profileID: String?, libraryStore: LibraryStore) {
        _store = State(initialValue: CatalogStore(api: api, category: category, profileID: profileID))
        self.profileID = profileID
        self.libraryStore = libraryStore
    }

    var body: some View {
        GeometryReader { viewport in
            ScrollView {
                Group {
                    if !store.hasLoaded && store.errorMessage == nil {
                        FeaturedCarouselSkeleton(viewportSize: viewport.size)
                            .padding(.vertical, 12)
                    } else if let errorMessage = store.errorMessage {
                        CatalogFailureView(message: errorMessage) {
                            Task { await store.load(force: true) }
                        }
                    } else if store.sections.isEmpty && store.featuredTitles.isEmpty {
                        ContentUnavailableView {
                            Label {
                                Text("No titles available")
                            } icon: {
                                PhosphorIcon("film-slate", size: 48)
                            }
                        } description: {
                            Text("There are no titles to show right now.")
                        }
                        .padding(.top, 88)
                    } else {
                        LazyVStack(alignment: .leading, spacing: 28) {
                            // Featured Carousel (Showroom Hero Stage)
                            if !store.featuredTitles.isEmpty {
                                FeaturedCarouselView(
                                    titles: store.featuredTitles,
                                    viewportSize: viewport.size,
                                    onSelect: { title in
                                        selectedTitle = title
                                    },
                                    onPlay: { title in
                                        playFeatured(title)
                                    }
                                )
                            }

                            curatedHomeSections
                        }
                        .padding(.vertical, 12)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .background {
                    GeometryReader { content in
                        Color.clear.preference(
                            key: CatalogScrollOffsetKey.self,
                            value: content.frame(in: .named("catalogScroll")).minY
                        )
                    }
                }
            }
            .coordinateSpace(name: "catalogScroll")
            .scrollIndicators(.hidden)
            .onPreferenceChange(CatalogScrollOffsetKey.self) { heroScrollOffset = $0 }
        }
        .background { CatalogPageSurface(heroOffset: heroScrollOffset).ignoresSafeArea() }
        .toolbar(.hidden, for: .navigationBar)
        .task(id: foregroundRefreshGeneration) {
            let generation = foregroundRefreshGeneration
            let refreshing = generation > completedForegroundRefreshGeneration
            await store.load(force: refreshing)
            guard !Task.isCancelled else { return }
            if let profileID {
                if refreshing {
                    await libraryStore.refresh(for: profileID)
                } else {
                    await libraryStore.loadLibrary(for: profileID)
                }
            }
            if !Task.isCancelled { completedForegroundRefreshGeneration = generation }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .background {
                hasEnteredBackground = true
            } else if phase == .active && hasEnteredBackground {
                // Temporary interruptions such as Control Center don't refresh feeds.
                hasEnteredBackground = false
                foregroundRefreshGeneration += 1
            }
        }
        .onChange(of: profileID) { _, newID in
            cancelResume()
            preloadedRequest = nil
            store.updateProfileID(newID)
            if let newID {
                Task {
                    await libraryStore.loadLibrary(for: newID, forceReload: true)
                }
            }
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
        .onReceive(NotificationCenter.default.publisher(for: .melloTVRecommendationsChanged)) { notification in
            guard let changedProfile = notification.userInfo?["profileID"] as? String,
                  changedProfile == profileID,
                  let titleID = notification.userInfo?["titleID"] as? String,
                  let choice = notification.userInfo?["choice"] as? String,
                  let enabled = notification.userInfo?["enabled"] as? Bool else { return }
            if enabled && ["hidden", "watched", "favorites"].contains(choice) {
                store.removeRecommendation(titleID)
            } else if !enabled {
                Task { await store.load(force: true) }
            }
        }
        .onDisappear(perform: cancelResume)
    }

    private func playFeatured(_ title: MediaTitle) {
        preloadTask?.cancel()
        guard let profileID else {
            selectedTitle = title
            return
        }

        if title.type == .movie {
            playbackRequest = PlaybackRequest(
                titleID: title.tmdbID,
                type: .movie,
                title: title.title,
                profileID: profileID,
                catalogTitle: title
            ).traced(.play)
        } else {
            // Resume an existing series, or start its first episode directly.
            if let saved = libraryStore.savedProgress(for: title) {
                resumeItem(saved)
            } else {
                playbackRequest = PlaybackRequest(
                    titleID: title.tmdbID,
                    type: .tv,
                    title: title.title,
                    profileID: profileID,
                    season: 1,
                    episode: 1,
                    catalogTitle: title
                ).traced(.play)
            }
        }
    }

    private func cancelResume() {
        preloadTask?.cancel()
        preloadTask = nil
        resumeTask?.cancel()
        resumeTask = nil
        resolvingTitleID = nil
    }

    private func scheduleContinueWatchingPreload(for items: [WatchProgress]) {
        guard let first = items.first, let profileID, resolvingTitleID == nil else { return }
        if preloadedRequest?.item == first && preloadedRequest?.request.profileID == profileID { return }
        preloadTask?.cancel()
        preloadTask = Task { @MainActor in
            do {
                try await Task.sleep(for: .milliseconds(400))
                try Task.checkCancellation()
                let request = try await ResumePlaybackHelper.resolveResumeRequest(for: first, profileID: profileID)
                try Task.checkCancellation()
                preloadedRequest = (item: first, request: request)
                PreparedPlaybackService.shared.preload(request: request)
            } catch {
                // Cancelled or network preloading quiet fallback
            }
        }
    }

    private func resumeItem(_ item: WatchProgress) {
        guard let profileID, resolvingTitleID == nil else { return }
        let timing = PlaybackTiming(action: .resume, titleID: item.title.tmdbID, type: item.title.type,
                                    season: item.season, episode: item.episode)
        preloadTask?.cancel()

        // Fast-path: if preloaded for this exact title, launch immediately with 0ms delay!
        if let preloaded = preloadedRequest, preloaded.item == item, preloaded.request.profileID == profileID {
            preloadedRequest = nil
            playbackRequest = preloaded.request.traced(.resume, using: timing)
            return
        }

        resolvingTitleID = item.title.id
        resumeTask = Task { @MainActor in
            defer {
                if !Task.isCancelled {
                    resolvingTitleID = nil
                    resumeTask = nil
                }
            }
            do {
                let request = try await ResumePlaybackHelper.resolveResumeRequest(for: item, profileID: profileID, timing: timing)
                try Task.checkCancellation()
                playbackRequest = request
            } catch {
                if !Task.isCancelled { selectedTitle = item.title }
            }
        }
    }

    @ViewBuilder
    private var curatedHomeSections: some View {
        // Continue Watching Shelf
        let continueItems = libraryStore.continueWatching(filter: store.category.mediaTypeFilter)
        if !continueItems.isEmpty {
            ContinueWatchingShelf(
                items: continueItems,
                resolvingTitleID: resolvingTitleID,
                onPlay: { item in
                    resumeItem(item)
                },
                onInfo: { title in
                    preloadTask?.cancel()
                    selectedTitle = title
                }
            )
            .onAppear {
                scheduleContinueWatchingPreload(for: continueItems)
            }
            .onChange(of: continueItems.first) { _, _ in
                scheduleContinueWatchingPreload(for: continueItems)
            }
        }

        // Dynamic Curated Shelves
        // SwiftUI may finish updating outgoing rows after a profile change
        // clears the store. Capture each section instead of retaining its index.
        ForEach(Array(store.sections.enumerated()), id: \.element.id) { index, section in
            if index == 0 && section.titles.count >= 4 {
                // Section 0: Numbered Top 10 Charts
                NumberedShelf(
                    section: section,
                    eyebrow: store.category == .tv ? "Top Shows Today" : "Top 10 Today",
                    onSelect: { selectedTitle = $0 }
                )
            } else if index == 1 {
                // Section 1: Panoramic 16:9 Cinema Shelf
                PanoramicShelf(
                    section: section,
                    eyebrow: "Cinematic Premieres",
                    onSelect: { selectedTitle = $0 }
                )

                // Editorial Spotlight Card Break
                if let spotlightPick = store.featuredTitles.dropFirst().first {
                    EditorialSpotlightCard(
                        title: spotlightPick,
                        onSelect: { selectedTitle = spotlightPick },
                        onPlay: { playFeatured(spotlightPick) }
                    )
                    .padding(.vertical, 4)
                }
            } else if index % 2 == 1 {
                // Alternating: Panoramic 16:9
                PanoramicShelf(
                    section: section,
                    onSelect: { selectedTitle = $0 }
                )
            } else {
                // Alternating: Classic 2:3 Posters
                CatalogShelf(
                    section: section,
                    onSelect: { selectedTitle = $0 }
                )
            }
        }
    }
}

// MARK: - Subviews

private struct NumberedShelf: View {
    let section: CatalogSection
    var eyebrow: String? = "TOP CHARTS"
    let onSelect: (MediaTitle) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(eyebrow ?? section.heading)
                .font(.title3.weight(.bold))
                .foregroundStyle(MelloTVTheme.text)
                .padding(.horizontal, 16)

            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(alignment: .top, spacing: 16) {
                    ForEach(Array(section.titles.prefix(10).enumerated()), id: \.element.id) { index, title in
                        NumberedPosterTile(rank: index + 1, title: title) {
                            onSelect(title)
                        }
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 4)
            }
            .scrollClipDisabled()
        }
    }
}

private struct NumberedPosterTile: View {
    let rank: Int
    let title: MediaTitle
    var action: (() -> Void)? = nil

    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    private var posterWidth: CGFloat { dynamicTypeSize.isAccessibilitySize ? 176 : 136 }
    private var posterHeight: CGFloat { posterWidth * 1.5 }

    var body: some View {
        if let action {
            Button(action: action) {
                content
                    .contentShape(Rectangle())
            }
            .buttonStyle(CardScaleButtonStyle(scale: 0.97))
            .accessibilityLabel("#\(rank), \(title.title)")
            .accessibilityHint("Shows title details")
        } else {
            content
                .accessibilityLabel("#\(rank), \(title.title)")
        }
    }

    private var content: some View {
        HStack(alignment: .bottom, spacing: -18) {
            Text("\(rank)")
                .font(.system(size: dynamicTypeSize.isAccessibilitySize ? 96 : 80, weight: .black, design: .rounded))
                .monospacedDigit()
                .foregroundStyle(MelloTVTheme.textSecondary.opacity(0.55))
                .offset(y: 8)
                .zIndex(0)
                .allowsHitTesting(false)

            posterArtwork
                .frame(width: posterWidth, height: posterHeight)
                .artworkCardSurface()
                .zIndex(1)
        }
        .frame(height: posterHeight)
        .padding(.trailing, 4)
    }

    @ViewBuilder
    private var posterArtwork: some View {
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
                    MelloTVTheme.cardBackground
                @unknown default:
                    placeholder
                }
            }
        } else {
            placeholder
        }
    }

    private var placeholder: some View {
        ZStack {
            MelloTVTheme.secondaryBackground

            VStack(spacing: 6) {
                PhosphorIcon(title.type == .movie ? "film-slate" : "television-simple", size: 24)
                    .foregroundStyle(MelloTVTheme.muted)
                Text(title.title)
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(MelloTVTheme.muted)
                    .lineLimit(2)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 6)
            }
        }
    }
}

private struct PanoramicShelf: View {
    let section: CatalogSection
    var eyebrow: String? = nil
    let onSelect: (MediaTitle) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(eyebrow ?? section.heading)
                .font(.title3.weight(.bold))
                .foregroundStyle(MelloTVTheme.text)
                .padding(.horizontal, 16)

            if let reason = section.reason, reason == "liked" || reason == "watched" {
                Text(reason == "liked" ? "Based on a title you liked" : "Based on a title you watched")
                    .font(.caption).foregroundStyle(MelloTVTheme.muted).padding(.horizontal, 16)
            }
            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(alignment: .top, spacing: 14) {
                    ForEach(section.titles) { title in
                        PanoramicCard(title: title) {
                            onSelect(title)
                        }
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 4)
            }
            .scrollClipDisabled()
        }
    }
}

private struct PanoramicCard: View {
    let title: MediaTitle
    var action: (() -> Void)? = nil

    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    private var cardWidth: CGFloat { verticalSizeClass == .compact ? 320 : 288 }
    private var cardHeight: CGFloat { cardWidth * (dynamicTypeSize.isAccessibilitySize ? 0.80 : 9.0 / 16) }

    var body: some View {
        if let action {
            Button(action: action) {
                cardBody
                    .contentShape(Rectangle())
            }
            .buttonStyle(CardScaleButtonStyle(scale: 0.97))
            .accessibilityLabel(accessibilityTitle)
            .accessibilityHint("Shows title details")
        } else {
            cardBody
                .accessibilityLabel(accessibilityTitle)
        }
    }

    private var cardBody: some View {
        ZStack(alignment: .bottomLeading) {
            artwork
                .frame(width: cardWidth, height: cardHeight)
                .clipped()

            LinearGradient(
                colors: [
                    Color.clear,
                    Color.black.opacity(0.35),
                    Color.black.opacity(0.85)
                ],
                startPoint: .center,
                endPoint: .bottom
            )
            .allowsHitTesting(false)

            Text(title.title)
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.white)
                .multilineTextAlignment(.leading)
                .lineLimit(2)
                .shadow(color: Color.black.opacity(0.35), radius: 3, y: 1)
                .padding(16)
        }
        .frame(width: cardWidth, height: cardHeight)
        .artworkCardSurface(cornerRadius: 20)
    }

    @ViewBuilder
    private var artwork: some View {
        let imageURL = title.backdropURL ?? title.posterURL
        if let imageURL {
            AsyncImage(url: imageURL) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .scaledToFill()
                case .failure:
                    placeholder
                case .empty:
                    MelloTVTheme.cardBackground
                @unknown default:
                    placeholder
                }
            }
        } else {
            placeholder
        }
    }

    private var placeholder: some View {
        ZStack {
            MelloTVTheme.secondaryBackground

            VStack(spacing: 8) {
                PhosphorIcon(title.type == .movie ? "film-slate" : "television-simple", size: 24)
                    .foregroundStyle(MelloTVTheme.muted)
                Text(title.title)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(MelloTVTheme.muted)
                    .lineLimit(1)
                    .padding(.horizontal, 8)
            }
        }
    }

    private var accessibilityTitle: String {
        var elements = [title.title]
        if !title.year.isEmpty { elements.append(title.year) }
        elements.append(title.type == .movie ? "Movie" : "Series")
        if title.rating > 0 {
            elements.append("Rating \(title.rating.formatted(.number.precision(.fractionLength(1))))")
        }
        return elements.joined(separator: ", ")
    }
}

private struct EditorialSpotlightCard: View {
    let title: MediaTitle
    let onSelect: () -> Void
    let onPlay: () -> Void

    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        Button(action: onPlay) {
            VStack(alignment: .leading, spacing: 0) {
                Spacer(minLength: verticalSizeClass == .compact ? 100 : 130)
                VStack(alignment: .leading, spacing: 6) {
                    Text("SPOTLIGHT")
                        .font(.caption2.weight(.semibold))
                        .tracking(1.6)
                        .foregroundStyle(.white.opacity(0.75))
                    Text(title.title)
                        .font(.title2.weight(.semibold))
                        .tracking(-0.4)
                        .foregroundStyle(.white)
                        .multilineTextAlignment(.leading)
                        .lineLimit(dynamicTypeSize.isAccessibilitySize ? nil : 2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(.leading, 20)
                .padding(.trailing, 68)
                .padding(.top, 32)
                .padding(.bottom, 20)
            }
            .frame(maxWidth: .infinity, minHeight: verticalSizeClass == .compact ? 200 : 240, alignment: .bottomLeading)
            .background {
                GeometryReader { geometry in
                    artwork
                        .frame(width: geometry.size.width, height: geometry.size.height)
                        .clipped()
                        .overlay {
                            LinearGradient(stops: [
                                .init(color: .clear, location: 0.25),
                                .init(color: .black.opacity(0.25), location: 0.50),
                                .init(color: .black.opacity(0.88), location: 1)
                            ], startPoint: .top, endPoint: .bottom)
                        }
                }
            }
            .contentShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Play \(title.title)")
        .overlay(alignment: .topTrailing) {
            BrandTVMark().padding(.trailing, 16).padding(.top, 16)
        }
        .overlay(alignment: .bottomTrailing) {
            Button(action: onSelect) { CardDetailsGlyph() }
                .buttonStyle(CardScaleButtonStyle(scale: 0.94))
                .accessibilityLabel("Details for \(title.title)")
                .padding(.trailing, 16)
                .padding(.bottom, 12)
        }
        .clipShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 0.5)
                .allowsHitTesting(false)
        }
        .padding(.horizontal, 16)
    }

    @ViewBuilder
    private var artwork: some View {
        let imageURL = title.backdropURL ?? title.posterURL
        if let imageURL {
            AsyncImage(url: imageURL) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .scaledToFill()
                case .failure:
                    placeholder
                case .empty:
                    ZStack {
                        MelloTVTheme.cardBackground
                        ProgressView()
                            .tint(MelloTVTheme.accent)
                    }
                @unknown default:
                    placeholder
                }
            }
        } else {
            placeholder
        }
    }

    private var placeholder: some View {
        ZStack {
            MelloTVTheme.secondaryBackground
            PhosphorIcon(title.type == .tv ? "television-simple" : "film-slate", size: 36)
                .foregroundStyle(MelloTVTheme.muted)
        }
    }
}

private struct CatalogShelf: View {
    let section: CatalogSection
    let onSelect: (MediaTitle) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(section.heading)
                .font(.title3.weight(.bold))
                .foregroundStyle(MelloTVTheme.text)
                .padding(.horizontal, 16)

            if let reason = section.reason, reason == "liked" || reason == "watched" {
                Text(reason == "liked" ? "Based on a title you liked" : "Based on a title you watched")
                    .font(.caption).foregroundStyle(MelloTVTheme.muted).padding(.horizontal, 16)
            }
            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(alignment: .top, spacing: 14) {
                    ForEach(section.titles) { title in
                        PosterTile(title: title) { onSelect(title) }
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 4)
            }
            .scrollClipDisabled()
        }
    }
}

private struct CatalogFailureView: View {
    let message: String
    let retry: () -> Void

    var body: some View {
        ContentUnavailableView {
            Label {
                Text("Catalog unavailable")
            } icon: {
                PhosphorIcon("wifi-slash", size: 48)
            }
        } description: {
            Text(message)
        } actions: {
            Button("Try again", action: retry)
                .buttonStyle(.borderedProminent)
                .tint(MelloTVTheme.accent)
        }
        .padding(.top, 68)
    }
}

private struct CatalogScrollOffsetKey: PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = nextValue() }
}
