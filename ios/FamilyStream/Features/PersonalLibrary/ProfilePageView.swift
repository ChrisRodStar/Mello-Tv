import SwiftUI

struct ProfilePageView: View {
    @Bindable var profileStore: ProfileStore
    var libraryStore: LibraryStore
    var onBrowseTitles: (() -> Void)? = nil

    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @AppStorage("app_appearance_mode") private var appearanceMode: AppearanceMode = .system
    @State private var selectedTitleForDetails: MediaTitle?
    @State private var playbackRequest: PlaybackRequest?
    @State private var showPreferenceSetup = false
    @State private var resumeTask: Task<Void, Never>?
    @State private var resolvingItem: ProfileProgressID?

    private var hasLibrary: Bool {
        !libraryStore.library.list.isEmpty || !libraryStore.library.progress.isEmpty
    }

    var body: some View {
        NavigationStack {
            GeometryReader { geometry in
                ScrollView {
                    if let profile = profileStore.activeProfile {
                        VStack(alignment: .leading, spacing: 28) {
                            identityHeader(profile)
                                .padding(.horizontal, 16)
                            libraryStatus(profile)
                                .padding(.horizontal, 16)
                            if showsCollections {
                                overview(profile)
                            }
                            appearanceSettingsSection
                                .padding(.horizontal, 16)
                        }
                        .padding(.top, verticalSizeClass == .compact ? 16 : 28)
                        .padding(.bottom, 28)
                    } else {
                        Text("No profile selected")
                            .foregroundStyle(FamilyStreamTheme.muted)
                            .padding()
                    }
                }
                .refreshable { await refreshLibrary() }
            }
            .background(FamilyStreamTheme.background.ignoresSafeArea())
            .navigationBarTitleDisplayMode(.inline)
            .toolbar(.hidden, for: .navigationBar)
            .navigationDestination(for: ProfileCollection.self) { collection in
                collectionPage(collection)
            }
        }
        .tint(FamilyStreamTheme.text)
        .task(id: profileStore.activeProfile?.id) {
            if let profile = profileStore.activeProfile {
                await libraryStore.loadLibrary(for: profile.id)
            }
        }
        .onDisappear {
            resumeTask?.cancel()
            resumeTask = nil
            resolvingItem = nil
        }
        .sheet(item: $selectedTitleForDetails) { title in
            if let profile = profileStore.activeProfile {
                TitleDetailSheet(title: title, profileID: profile.id, libraryStore: libraryStore) { request in
                    playbackRequest = request
                }
            }
        }
        .fullScreenCover(item: $playbackRequest) { request in
            PlayerView(request: request, onDismiss: {
                playbackRequest = nil
                Task { await refreshLibrary() }
            })
        }
        .fullScreenCover(isPresented: $showPreferenceSetup) {
            if let profile = profileStore.activeProfile {
                OnboardingView(profileStore: profileStore, profile: profile) {
                    showPreferenceSetup = false
                }
            }
        }
    }

    private func identityHeader(_ profile: Profile) -> some View {
        HStack(alignment: .center, spacing: 14) {
            ZStack(alignment: .bottomTrailing) {
                ProfileAvatarView(name: profile.name, colorHex: profile.color, size: 76)
                Button { showPreferenceSetup = true } label: {
                    PhosphorIcon("gear-six", size: 14, weight: .fill)
                        .foregroundStyle(FamilyStreamTheme.text)
                        .frame(width: 30, height: 30)
                        .appleNeumorphic(cornerRadius: 15, elevation: .subtle)
                        .touchTarget()
                }
                .buttonStyle(CardScaleButtonStyle(scale: 0.94))
                .offset(x: 4, y: 4)
                .accessibilityLabel("Viewing preferences")
                .accessibilityHint("Edit favorite titles and genres for recommendations")
            }
            VStack(alignment: .leading, spacing: 4) {
                Text(profile.name)
                    .font(.largeTitle.weight(.bold))
                    .foregroundStyle(FamilyStreamTheme.text)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                Text("Your library")
                    .font(.subheadline)
                    .foregroundStyle(FamilyStreamTheme.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            Button(action: profileStore.switchProfile) {
                HStack(spacing: 6) {
                    PhosphorIcon("arrows-left-right", size: 13, weight: .bold)
                    Text("Switch")
                        .font(.subheadline.weight(.semibold))
                }
                .foregroundStyle(FamilyStreamTheme.text)
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .background(FamilyStreamTheme.cardBackground, in: Capsule())
                .overlay(
                    Capsule().stroke(FamilyStreamTheme.border, lineWidth: 0.8)
                )
                .shadow(color: Color.black.opacity(0.06), radius: 3, y: 1)
            }
            .buttonStyle(CardScaleButtonStyle(scale: 0.96))
            .accessibilityLabel("Switch profile")
        }
    }

    @ViewBuilder
    private func libraryStatus(_ profile: Profile) -> some View {
        if libraryStore.isLoading && !hasLibrary {
            ProgressView("Loading your library")
                .font(.subheadline)
                .tint(FamilyStreamTheme.accent)
                .frame(maxWidth: .infinity, minHeight: 80)
        } else if let message = libraryStore.errorMessage {
            VStack(alignment: .leading, spacing: 8) {
                Text(message)
                    .font(.subheadline)
                    .foregroundStyle(FamilyStreamTheme.muted)
                Button("Try again") {
                    Task { await libraryStore.loadLibrary(for: profile.id, forceReload: true) }
                }
                .font(.subheadline.weight(.semibold))
                .touchTarget()
            }
        }
    }

    private func overview(_ profile: Profile) -> some View {
        VStack(alignment: .leading, spacing: 32) {
            VStack(alignment: .leading, spacing: 12) {
                sectionHeading(.continueWatching, count: libraryStore.continueWatching().count)
                let items = libraryStore.continueWatching()
                if items.isEmpty {
                    emptyState(.continueWatching, compact: true).padding(.horizontal, 16)
                } else {
                    ContinueWatchingShelf(
                        items: Array(items.prefix(6)),
                        showsHeading: false,
                        resolvingTitleID: resolvingItem?.titleID,
                        onPlay: { resume($0, profile: profile) },
                        onInfo: { selectedTitleForDetails = $0 }
                    )
                }
            }
            VStack(alignment: .leading, spacing: 12) {
                sectionHeading(.myList, count: libraryStore.library.list.count)
                if libraryStore.library.list.isEmpty {
                    emptyState(.myList, compact: true).padding(.horizontal, 16)
                } else {
                    ScrollView(.horizontal) {
                        LazyHStack(alignment: .top, spacing: 8) {
                            ForEach(Array(libraryStore.library.list.prefix(10))) { title in
                                savedTile(title).frame(width: dynamicTypeSize.isAccessibilitySize ? 148 : 108)
                            }
                        }
                        .padding(.horizontal, 16)
                    }
                    .scrollIndicators(.hidden)
                }
            }
            VStack(alignment: .leading, spacing: 12) {
                sectionHeading(.history, count: libraryStore.watchHistory.count)
                if libraryStore.watchHistory.isEmpty {
                    emptyState(.history, compact: true).padding(.horizontal, 16)
                } else {
                    VStack(spacing: 10) {
                        ForEach(Array(libraryStore.watchHistory.prefix(3)), id: \.profileID) { item in
                            historyRow(item, profile: profile)
                        }
                    }
                    .padding(.horizontal, 16)
                }
            }
        }
    }

    private func sectionHeading(_ collection: ProfileCollection, count: Int) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text(collection.rawValue)
                .font(.title3.weight(.bold))
                .foregroundStyle(FamilyStreamTheme.text)
                .accessibilityAddTraits(.isHeader)
            Spacer(minLength: 0)
            NavigationLink(value: collection) {
                HStack(spacing: 5) {
                    Text("View all")
                    PhosphorIcon("caret-right", size: 12, weight: .bold)
                }
                .font(.caption.weight(.semibold))
                .foregroundStyle(FamilyStreamTheme.textSecondary)
                .fixedSize(horizontal: true, vertical: false)
                .frame(minHeight: 44)
                .contentShape(Rectangle())
            }
            .accessibilityLabel("View all \(collection.rawValue), \(count) items")
        }
        .padding(.horizontal, 16)
    }

    private func collectionPage(_ collection: ProfileCollection) -> some View {
        ScrollView {
            if let profile = profileStore.activeProfile {
                VStack(alignment: .leading, spacing: 20) {
                    libraryStatus(profile)
                    if showsCollections {
                        collectionContent(collection, profile: profile)
                    }
                }
                .padding(16)
                .padding(.bottom, 12)
            }
        }
        .refreshable { await refreshLibrary() }
        .background(FamilyStreamTheme.background.ignoresSafeArea())
        .navigationTitle(collection.rawValue)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar(.visible, for: .navigationBar)
    }

    private var appearanceSettingsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Appearance")
                .font(.title3.weight(.bold))
                .foregroundStyle(FamilyStreamTheme.text)

            VStack(spacing: 0) {
                Picker("Appearance Mode", selection: $appearanceMode) {
                    ForEach(AppearanceMode.allCases) { mode in
                        Label(mode.rawValue, image: mode.iconName)
                            .tag(mode)
                    }
                }
                .pickerStyle(.segmented)
                .padding(8)
            }
            .appleNeumorphic(cornerRadius: 14, elevation: .subtle)
        }
    }

    private var showsCollections: Bool {
        hasLibrary || (!libraryStore.isLoading && libraryStore.errorMessage == nil)
    }

    @ViewBuilder
    private func collectionContent(_ collection: ProfileCollection, profile: Profile) -> some View {
        switch collection {
        case .myList:
            if libraryStore.library.list.isEmpty {
                emptyState(collection)
            } else {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: dynamicTypeSize.isAccessibilitySize ? 148 : 108), spacing: 8)],
                          alignment: .leading, spacing: 12) {
                    ForEach(libraryStore.library.list) { title in savedTile(title) }
                }
            }
        case .continueWatching:
            let items = libraryStore.continueWatching()
            if items.isEmpty {
                emptyState(collection)
            } else {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: dynamicTypeSize.isAccessibilitySize ? 180 : 110), spacing: 12)],
                          alignment: .leading, spacing: 24) {
                    ForEach(items, id: \.profileID) { item in progressCard(item, profile: profile) }
                }
            }
        case .history:
            if libraryStore.watchHistory.isEmpty {
                emptyState(collection)
            } else {
                LazyVStack(spacing: 10) {
                    ForEach(libraryStore.watchHistory, id: \.profileID) { item in historyRow(item, profile: profile) }
                }
            }
        }
    }

    private func progressCard(_ item: WatchProgress, profile: Profile) -> some View {
        LandscapeCard(
            item: item,
            isPreparingPlayback: resolvingItem == item.profileID,
            isPlaybackDisabled: resolvingItem != nil,
            onPlay: { resume(item, profile: profile) },
            onInfo: { selectedTitleForDetails = item.title }
        )
    }

    private func historyRow(_ item: WatchProgress, profile: Profile) -> some View {
        Group {
            if dynamicTypeSize.isAccessibilitySize {
                VStack(alignment: .leading, spacing: 10) {
                    playbackArtwork(item, profile: profile)
                    detailsButton(item, showsDate: true)
                }
            } else {
                HStack(alignment: .center, spacing: 12) {
                    playbackArtwork(item, profile: profile)
                        .frame(width: 116)

                    detailsButton(item, showsDate: true)
                }
            }
        }
        .padding(10)
        .background(FamilyStreamTheme.cardBackground, in: RoundedRectangle(cornerRadius: 10))
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .stroke(FamilyStreamTheme.subtleBorder, lineWidth: 0.5)
        )
        .shadow(color: Color.black.opacity(0.04), radius: 6, x: 0, y: 2)
    }

    private func playbackArtwork(_ item: WatchProgress, profile: Profile) -> some View {
        Button { resume(item, profile: profile) } label: {
            ProfileArtwork(title: item.title, isPoster: false)
                .overlay { playbackGlyph(item) }
                .overlay(alignment: .bottom) { progressBar(item) }
                .clipShape(.rect(cornerRadius: 6))
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(FamilyStreamTheme.subtleBorder, lineWidth: 0.5)
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.98))
        .disabled(resolvingItem != nil)
        .accessibilityLabel("\(item.playbackAction) \(item.title.title)")
        .accessibilityValue(resolvingItem == item.profileID ? "Preparing playback" : item.profileSummary)
    }

    private func detailsButton(_ item: WatchProgress, showsDate: Bool = false) -> some View {
        Button { selectedTitleForDetails = item.title } label: {
            HStack(alignment: .center, spacing: 8) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(item.title.title)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(FamilyStreamTheme.text)
                        .lineLimit(1)

                    Text(item.profileSummary)
                        .font(.caption)
                        .foregroundStyle(FamilyStreamTheme.muted)
                        .lineLimit(1)

                    if showsDate, item.updatedAt.isFinite, item.updatedAt > 0 {
                        Text(Date(timeIntervalSince1970: item.updatedAt), style: .date)
                            .font(.caption2)
                            .foregroundStyle(FamilyStreamTheme.mutedSubtle)
                    }
                }
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)

                PhosphorIcon("caret-right", size: 12, weight: .bold)
                    .foregroundStyle(FamilyStreamTheme.muted.opacity(0.6))
                    .accessibilityHidden(true)
            }
            .multilineTextAlignment(.leading)
            .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityHint("Shows title details")
    }

    private func savedTile(_ title: MediaTitle) -> some View {
        Button { selectedTitleForDetails = title } label: {
            ProfileArtwork(title: title, isPoster: true)
                .clipShape(.rect(cornerRadius: 6))
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(FamilyStreamTheme.subtleBorder, lineWidth: 0.5)
                )
                .shadow(color: .black.opacity(0.4), radius: 4, x: 0, y: 2)
                .contentShape(Rectangle())
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.96))
        .accessibilityLabel("\(title.title), \(title.type == .movie ? "Movie" : "Series"), \(title.year)")
        .accessibilityHint("Shows title details and My List controls")
    }

    private func playbackGlyph(_ item: WatchProgress) -> some View {
        Group {
            if resolvingItem == item.profileID {
                ProgressView().tint(.white)
            } else {
                PhosphorIcon("play", size: 18, weight: .fill)
                    .foregroundStyle(.white)
            }
        }
        .shadow(color: .black.opacity(0.8), radius: 4)
        .accessibilityHidden(true)
    }

    @ViewBuilder
    private func progressBar(_ item: WatchProgress) -> some View {
        if let fraction = item.profileFraction {
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Rectangle().fill(.black.opacity(0.6))
                    Rectangle()
                        .fill(item.completed ? FamilyStreamTheme.muted : FamilyStreamTheme.accent)
                        .frame(width: geometry.size.width * fraction)
                }
            }
            .frame(height: 4)
            .accessibilityHidden(true)
        }
    }

    private func emptyState(_ collection: ProfileCollection, compact: Bool = false) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            if !compact {
                PhosphorIcon(collection.symbol, size: 36)
                    .foregroundStyle(FamilyStreamTheme.muted)
                    .padding(.bottom, 8)
                    .accessibilityHidden(true)
            }
            Text(collection.emptyTitle)
                .font(.headline)
                .foregroundStyle(FamilyStreamTheme.text)
            Text(collection.emptyMessage)
                .font(.subheadline)
                .foregroundStyle(FamilyStreamTheme.muted)
                .fixedSize(horizontal: false, vertical: true)
            if let onBrowseTitles, !compact || collection == .myList {
                Button("Browse titles", action: onBrowseTitles)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(FamilyStreamTheme.background)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .frame(minHeight: 44)
                    .background(.white, in: .rect(cornerRadius: 6))
                    .buttonStyle(CardScaleButtonStyle(scale: 0.98))
                    .padding(.top, 4)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, compact ? 4 : 28)
    }

    private func refreshLibrary() async {
        if let profile = profileStore.activeProfile { await libraryStore.refresh(for: profile.id) }
    }

    private func resume(_ item: WatchProgress, profile: Profile) {
        guard resolvingItem == nil else { return }
        let timing = PlaybackTiming(action: .resume, titleID: item.title.tmdbID, type: item.title.type,
                                    season: item.season, episode: item.episode)
        resolvingItem = item.profileID
        resumeTask = Task { @MainActor in
            defer { resolvingItem = nil; resumeTask = nil }
            do {
                let request = try await ResumePlaybackHelper.resolveResumeRequest(for: item, profileID: profile.id, timing: timing)
                try Task.checkCancellation()
                guard profileStore.activeProfile?.id == profile.id else { return }
                playbackRequest = request
            } catch {
                guard !Task.isCancelled, profileStore.activeProfile?.id == profile.id else { return }
                selectedTitleForDetails = item.title
            }
        }
    }
}

private enum ProfileCollection: String, Hashable {
    case continueWatching = "Continue Watching"
    case myList = "My List"
    case history = "History"

    var symbol: String {
        switch self {
        case .continueWatching: "play"
        case .myList: "bookmark-simple"
        case .history: "clock-counter-clockwise"
        }
    }

    var emptyTitle: String {
        switch self {
        case .continueWatching: "Ready when you are"
        case .myList: "Save something for later"
        case .history: "Your viewing starts here"
        }
    }

    var emptyMessage: String {
        switch self {
        case .continueWatching: "Pick a movie or series. You can return to it here."
        case .myList: "Tap the bookmark on any title to add it to My List."
        case .history: "The movies and episodes you watch will appear here."
        }
    }
}

private struct ProfileArtwork: View {
    let title: MediaTitle
    let isPoster: Bool

    var body: some View {
        Color.clear
            .aspectRatio(isPoster ? 2.0 / 3 : 16.0 / 9, contentMode: .fit)
            .overlay {
                GeometryReader { geometry in
                    AsyncImage(url: isPoster ? title.posterURL : title.backdropURL ?? title.posterURL) { phase in
                        if case .success(let image) = phase {
                            image.resizable().scaledToFill()
                        } else {
                            FamilyStreamTheme.cardBackground
                                .overlay {
                                    PhosphorIcon(title.type == .movie ? "film-slate" : "television-simple", size: 24)
                                        .foregroundStyle(FamilyStreamTheme.muted)
                                }
                        }
                    }
                    .frame(width: geometry.size.width, height: geometry.size.height)
                    .clipped()
                    .overlay(.black.opacity(isPoster ? 0 : 0.22))
                }
            }
            .accessibilityHidden(true)
    }
}

// History contains individual episodes; title ID alone is not a stable row identity.
private struct ProfileProgressID: Hashable {
    let titleID: String
    let season: Int
    let episode: Int
}

private extension WatchProgress {
    var profileID: ProfileProgressID {
        ProfileProgressID(titleID: title.id, season: season, episode: episode)
    }

    var playbackAction: String {
        completed ? (title.type == .tv ? "Continue" : "Play") : "Resume"
    }

    var profileFraction: Double? {
        guard duration.isFinite, position.isFinite, duration > 0 else { return nil }
        return min(1, max(0, position / duration))
    }

    var profileSummary: String {
        var parts: [String] = []
        if title.type == .tv { parts.append("S\(season) · E\(episode)") }
        if !episodeTitle.isEmpty { parts.append(episodeTitle) }
        if completed {
            parts.append("Completed")
        } else if let fraction = profileFraction {
            let remaining = (duration * (1 - fraction) / 60).rounded(.up)
            if remaining < Double(Int.max) {
                parts.append("\(Int(remaining)) min left")
            }
        } else {
            parts.append("In progress")
        }
        return parts.joined(separator: " · ")
    }
}
