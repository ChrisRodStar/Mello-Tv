import SwiftUI

struct CatalogView: View {
    @State private var store: CatalogStore
    @State private var selectedTitle: MediaTitle?
    @State private var playbackRequest: PlaybackRequest?
    @State private var resolvingTitleID: String?
    @State private var resumeTask: Task<Void, Never>?
    @State private var preloadTask: Task<Void, Never>?
    @State private var preloadedRequest: (item: WatchProgress, request: PlaybackRequest)?

    let profileID: String?
    var libraryStore: LibraryStore

    init(api: FamilyStreamAPI, category: CatalogCategory, profileID: String?, libraryStore: LibraryStore) {
        _store = State(initialValue: CatalogStore(api: api, category: category, profileID: profileID))
        self.profileID = profileID
        self.libraryStore = libraryStore
    }

    var body: some View {
        ScrollView {
            Group {
                if store.isLoading && !store.hasLoaded {
                    CatalogLoadingView()
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
        }
        .background(FamilyStreamTheme.background.ignoresSafeArea())
        .scrollIndicators(.hidden)
        .toolbar(.hidden, for: .navigationBar)
        .task {
            await store.load()
            if let profileID {
                await libraryStore.loadLibrary(for: profileID)
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
        .refreshable {
            await store.load(force: true)
            if let profileID {
                await libraryStore.refresh(for: profileID)
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
            // For TV, if progress exists, resume it; otherwise open details to select episode
            if let saved = libraryStore.savedProgress(for: title) {
                resumeItem(saved)
            } else {
                selectedTitle = title
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
        ForEach(store.sections.indices, id: \.self) { index in
            let section = store.sections[index]
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
                if store.featuredTitles.count > 1 {
                    let spotlightPick = store.featuredTitles[1]
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
            VStack(alignment: .leading, spacing: 3) {
                if let eyebrow {
                    Text(eyebrow.uppercased())
                        .font(.caption2.weight(.bold))
                        .tracking(0.6)
                        .foregroundStyle(FamilyStreamTheme.muted)
                }

                Text(section.heading)
                    .font(.title3.weight(.bold))
                    .foregroundStyle(FamilyStreamTheme.text)
            }
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

    private let posterWidth: CGFloat = 126
    private let posterHeight: CGFloat = 189

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
                .font(.system(size: 88, weight: .black, design: .rounded))
                .foregroundStyle(
                    LinearGradient(
                        colors: [
                            FamilyStreamTheme.text.opacity(0.35),
                            FamilyStreamTheme.text.opacity(0.12)
                        ],
                        startPoint: .top,
                        endPoint: .bottom
                    )
                )
                .offset(x: rank >= 10 ? -10 : 0, y: 12)
                .zIndex(0)
                .allowsHitTesting(false)

            posterArtwork
                .frame(width: posterWidth, height: posterHeight)
                .clipShape(RoundedRectangle(cornerRadius: 11, style: .continuous))
                .appleNeumorphic(cornerRadius: 11, elevation: .medium)
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
                    ZStack {
                        FamilyStreamTheme.cardBackground
                        ProgressView()
                            .tint(FamilyStreamTheme.accent)
                            .scaleEffect(0.8)
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
            FamilyStreamTheme.secondaryBackground

            VStack(spacing: 6) {
                PhosphorIcon(title.type == .movie ? "film-slate" : "television-simple", size: 24)
                    .foregroundStyle(FamilyStreamTheme.muted)
                Text(title.title)
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(FamilyStreamTheme.muted)
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
            VStack(alignment: .leading, spacing: 3) {
                if let eyebrow {
                    Text(eyebrow.uppercased())
                        .font(.caption2.weight(.bold))
                        .tracking(0.6)
                        .foregroundStyle(FamilyStreamTheme.muted)
                }

                Text(section.heading)
                    .font(.title3.weight(.bold))
                    .foregroundStyle(FamilyStreamTheme.text)
            }
            .padding(.horizontal, 16)

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

    private let cardWidth: CGFloat = 260
    private let cardHeight: CGFloat = 146

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
                .font(.subheadline.weight(.bold))
                .foregroundStyle(.white)
                .lineLimit(1)
                .shadow(color: Color.black.opacity(0.8), radius: 3, x: 0, y: 1)
                .padding(.horizontal, 12)
                .padding(.bottom, 10)
        }
        .frame(width: cardWidth, height: cardHeight)
        .clipShape(RoundedRectangle(cornerRadius: 13, style: .continuous))
        .appleNeumorphic(cornerRadius: 13, elevation: .medium)
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
                        FamilyStreamTheme.cardBackground
                        ProgressView()
                            .tint(FamilyStreamTheme.accent)
                            .scaleEffect(0.8)
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
            FamilyStreamTheme.secondaryBackground

            VStack(spacing: 8) {
                PhosphorIcon(title.type == .movie ? "film-slate" : "television-simple", size: 24)
                    .foregroundStyle(FamilyStreamTheme.muted)
                Text(title.title)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(FamilyStreamTheme.muted)
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

    var body: some View {
        Button(action: onSelect) {
            ZStack(alignment: .topLeading) {
                artwork
                    .frame(height: 220)
                    .frame(maxWidth: .infinity)
                    .clipped()

                LinearGradient(
                    colors: [
                        Color.clear,
                        Color.black.opacity(0.45),
                        Color.black.opacity(0.88)
                    ],
                    startPoint: .center,
                    endPoint: .bottom
                )
                .allowsHitTesting(false)

                // Subtle top left-hand corner tag (no icon, just "Feature Spotlight")
                Text("Feature Spotlight")
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(Color.white.opacity(0.85))
                    .padding(.horizontal, 10)
                    .padding(.vertical, 5)
                    .background(Color.black.opacity(0.45), in: Capsule())
                    .overlay(
                        Capsule()
                            .stroke(Color.white.opacity(0.18), lineWidth: 0.5)
                    )
                    .padding(14)

                VStack(alignment: .leading, spacing: 8) {
                    Spacer()

                    Text(title.title)
                        .font(.title3.weight(.bold))
                        .foregroundStyle(.white)
                        .lineLimit(1)
                        .shadow(color: Color.black.opacity(0.8), radius: 4, x: 0, y: 2)

                    if !title.overview.isEmpty {
                        Text(title.overview)
                            .font(.caption)
                            .foregroundStyle(Color.white.opacity(0.82))
                            .lineLimit(2)
                            .lineSpacing(2)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }

                    HStack(spacing: 12) {
                        Button(action: onPlay) {
                            HStack(spacing: 6) {
                                PhosphorIcon("play", size: 12, weight: .fill)
                                Text("Play")
                                    .font(.subheadline.weight(.semibold))
                            }
                            .foregroundStyle(Color.black)
                            .padding(.horizontal, 18)
                            .padding(.vertical, 9)
                            .background(Color.white, in: Capsule())
                            .shadow(color: Color.black.opacity(0.2), radius: 4, x: 0, y: 2)
                        }
                        .buttonStyle(CardScaleButtonStyle(scale: 0.95))

                        Button(action: onSelect) {
                            Text("Details")
                                .font(.subheadline.weight(.medium))
                                .foregroundStyle(.white)
                                .padding(.horizontal, 16)
                                .padding(.vertical, 9)
                                .background(Color.white.opacity(0.18), in: Capsule())
                                .overlay(
                                    Capsule()
                                        .stroke(Color.white.opacity(0.22), lineWidth: 0.8)
                                )
                        }
                        .buttonStyle(CardScaleButtonStyle(scale: 0.95))
                    }
                    .padding(.top, 2)
                }
                .padding(16)
            }
            .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
            .appleNeumorphic(cornerRadius: 18, elevation: .prominent)
            .padding(.horizontal, 16)
        }
        .buttonStyle(.plain)
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
                        FamilyStreamTheme.cardBackground
                        ProgressView()
                            .tint(FamilyStreamTheme.accent)
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
            FamilyStreamTheme.secondaryBackground
            PhosphorIcon("film-slate", size: 36)
                .foregroundStyle(FamilyStreamTheme.muted)
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
                .foregroundStyle(FamilyStreamTheme.text)
                .padding(.horizontal, 16)

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

private struct CatalogLoadingView: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 28) {
            ForEach(0..<3, id: \.self) { _ in
                VStack(alignment: .leading, spacing: 12) {
                    RoundedRectangle(cornerRadius: 4)
                        .fill(FamilyStreamTheme.secondaryBackground)
                        .frame(width: 150, height: 22)
                    HStack(spacing: 14) {
                        ForEach(0..<4, id: \.self) { _ in
                            RoundedRectangle(cornerRadius: 8)
                                .fill(FamilyStreamTheme.secondaryBackground)
                                .frame(width: 126, height: 189)
                        }
                    }
                }
            }
        }
        .redacted(reason: .placeholder)
        .padding(.horizontal, 20)
        .padding(.top, 18)
        .accessibilityLabel("Loading catalog")
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
                .tint(FamilyStreamTheme.accent)
        }
        .padding(.top, 68)
    }
}
