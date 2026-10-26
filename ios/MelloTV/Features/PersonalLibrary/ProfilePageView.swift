import SwiftUI

struct ProfilePageView: View {
    @Bindable var profileStore: ProfileStore
    var libraryStore: LibraryStore
    var onBrowseTitles: (() -> Void)? = nil
    var onSwitchProfile: (() -> Void)? = nil

    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @Environment(\.openURL) private var openURL
    @AppStorage("app_appearance_mode") private var appearanceMode: AppearanceMode = .system
    @State private var selectedTitleForDetails: MediaTitle?
    @State private var playbackRequest: PlaybackRequest?
    @State private var showPreferenceSetup = false
    @State private var resumeTask: Task<Void, Never>?
    @State private var resolvingItem: ProfileProgressID?
    @State private var selectedCollection: ProfileCollection = .continueWatching
    @State private var continueScrollID: ProfileProgressID?
    @State private var savedScrollID: String?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var hasLibrary: Bool {
        !libraryStore.library.list.isEmpty || !libraryStore.library.progress.isEmpty
    }

    var body: some View {
        NavigationStack {
            GeometryReader { geometry in
                if let profile = profileStore.activeProfile {
                    profilePage(profile, size: geometry.size)
                } else {
                    Text("No profile selected")
                        .foregroundStyle(MelloTVTheme.muted)
                        .padding()
                }
            }
            .navigationBarTitleDisplayMode(.inline)
            .toolbar(.hidden, for: .navigationBar)
            .navigationDestination(for: ProfileCollection.self) { collection in
                collectionPage(collection)
            }
        }
        .background { profileSurface.ignoresSafeArea() }
        .presentationBackground(.clear)
        .tint(MelloTVTheme.text)
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

    private func profilePage(_ profile: Profile, size: CGSize) -> some View {
        Group {
            if size.width > size.height {
                HStack(alignment: .top, spacing: 12) {
                    identityHeader(profile)
                        .frame(width: min(300, size.width * 0.38))
                        .fixedSize(horizontal: false, vertical: true)
                    profileContent(profile, width: size.width - min(300, size.width * 0.38) - 12)
                }
            } else {
                VStack(alignment: .leading, spacing: 12) {
                    identityHeader(profile)
                        .fixedSize(horizontal: false, vertical: true)
                    profileContent(profile, width: size.width)
                }
            }
        }
        .padding(.top, 12)
        .padding(.bottom, 8)
    }

    private func profileContent(_ profile: Profile, width: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            libraryStatus(profile).padding(.horizontal, 24)
            sectionHeading(selectedCollection, count: collectionCount(selectedCollection), availableWidth: width)
            if showsCollections {
                if selectedCollection == .history {
                    ScrollView {
                        overview(profile, availableWidth: width)
                            .padding(.bottom, 24)
                    }
                    .scrollIndicators(.hidden)
                    .frame(maxHeight: .infinity)
                } else {
                    overview(profile, availableWidth: width)
                        .frame(maxHeight: .infinity)
                }
            } else {
                Spacer(minLength: 0)
            }
            if selectedCollection == .continueWatching {
                appearanceSettingsSection.padding(.horizontal, 24)
            }
        }
        .frame(maxHeight: .infinity, alignment: .top)
    }

    private func identityHeader(_ profile: Profile) -> some View {
        return VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .profileIdentityCenter, spacing: 20) {
                VStack(alignment: .leading, spacing: dynamicTypeSize.isAccessibilitySize ? 8 : -11) {
                    Text(profile.name)
                        .font(.largeTitle.weight(.bold))
                        .tracking(-1)
                        .foregroundStyle(MelloTVTheme.text)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityAddTraits(.isHeader)
                        .alignmentGuide(.profileIdentityCenter) { dimensions in dimensions[VerticalAlignment.center] }
                    Button { showPreferenceSetup = true } label: {
                        HStack(spacing: 5) {
                            Text("Viewing preferences")
                                .font(.caption.weight(.medium))
                            PhosphorIcon("caret-right", size: 10, weight: .bold)
                        }
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .frame(minHeight: 44, alignment: .leading)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityHint("Edit favorite titles and genres for recommendations")
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                Button {
                    if let onSwitchProfile { onSwitchProfile() }
                    else { profileStore.switchProfile() }
                } label: {
                    if #available(iOS 26.0, *) {
                        profileSwitchArtwork(profile)
                            .glassEffect(.regular.interactive(), in: Circle())
                    } else {
                        profileSwitchArtwork(profile)
                            .background(.regularMaterial, in: Circle())
                    }
                }
                .buttonStyle(CardScaleButtonStyle(scale: 0.96))
                .accessibilityLabel("Switch profile")
                .accessibilityValue(profile.name)
                .alignmentGuide(.profileIdentityCenter) { dimensions in dimensions[VerticalAlignment.center] }
            }
        }
        .padding(.horizontal, 24)
        .padding(.top, 8)
        .padding(.bottom, 12)
    }

    private func profileSwitchArtwork(_ profile: Profile) -> some View {
        ProfileAvatarView(name: profile.name, colorHex: profile.color, size: 80)
            .clipShape(Circle())
            .frame(width: 94, height: 94)
            .overlay(alignment: .bottomTrailing) {
                PhosphorIcon("arrows-left-right", size: 12)
                    .foregroundStyle(.white)
                    .frame(width: 25, height: 25)
                    .background(.black.opacity(0.65), in: Circle())
                    .overlay { Circle().strokeBorder(.white.opacity(0.20), lineWidth: 0.5) }
                    .padding(3)
                    .allowsHitTesting(false)
            }
            .contentShape(Circle())
    }

    private var profileSurface: some View {
        GeometryReader { geometry in
            ZStack(alignment: .top) {
                HeroSatinBackdrop(strength: 0.18, reactsToMotion: false)
                ZStack(alignment: .topTrailing) {
                    HeroSatinBackdrop()
                    if let profile = profileStore.activeProfile {
                        RadialGradient(
                            colors: [ProfileColor.color(for: profile.color).opacity(0.16), .clear],
                            center: .topTrailing, startRadius: 0, endRadius: 300
                        )
                    }
                }
                .frame(height: min(geometry.size.height, 460))
                .mask {
                    LinearGradient(stops: [
                        .init(color: .black, location: 0),
                        .init(color: .black, location: 0.55),
                        .init(color: .clear, location: 1)
                    ], startPoint: .top, endPoint: .bottom)
                }
            }
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    private func collectionCount(_ collection: ProfileCollection) -> Int {
        switch collection {
        case .continueWatching: libraryStore.continueWatching().count
        case .myList: libraryStore.library.list.count
        case .history: libraryStore.watchHistory.count
        }
    }

    @ViewBuilder
    private func libraryStatus(_ profile: Profile) -> some View {
        if libraryStore.isLoading && !hasLibrary {
            ProgressView("Loading your library")
                .font(.subheadline)
                .tint(MelloTVTheme.accent)
                .frame(maxWidth: .infinity, minHeight: 80)
        } else if let message = libraryStore.errorMessage {
            VStack(alignment: .leading, spacing: 8) {
                Text(message)
                    .font(.subheadline)
                    .foregroundStyle(MelloTVTheme.muted)
                Button("Try again") {
                    Task { await libraryStore.loadLibrary(for: profile.id, forceReload: true) }
                }
                .font(.subheadline.weight(.semibold))
                .touchTarget()
            }
        }
    }

    private func overview(_ profile: Profile, availableWidth: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            switch selectedCollection {
            case .continueWatching:
                let items = libraryStore.continueWatching()
                if items.isEmpty {
                    emptyState(.continueWatching, compact: true).padding(.horizontal, 24)
                } else {
                    ProfileAdaptivePosterShelf(
                        items: items, id: \.profileID, scrollID: $continueScrollID
                    ) { item, width in
                        ContinueWatchingCard(
                            item: item,
                            isPoster: true,
                            showsDetailsButton: false,
                            isPreparingPlayback: resolvingItem == item.profileID,
                            isPlaybackDisabled: resolvingItem != nil,
                            onPlay: { resume(item, profile: profile) },
                            onInfo: { selectedTitleForDetails = item.title }
                        )
                        .frame(width: width)
                    }
                }
            case .myList:
                if libraryStore.library.list.isEmpty {
                    emptyState(.myList, compact: true).padding(.horizontal, 24)
                } else {
                    ProfileAdaptivePosterShelf(
                        items: libraryStore.library.list, id: \.id, scrollID: $savedScrollID
                    ) { title, width in
                        savedTile(title).frame(width: width)
                    }
                }
            case .history:
                if libraryStore.watchHistory.isEmpty {
                    emptyState(.history, compact: true).padding(.horizontal, 24)
                } else {
                    historyJournal(profile)
                        .padding(.horizontal, 24)
                }
            }
        }
    }

    private var continueColumns: [ProfileContinueColumn] {
        let items = libraryStore.continueWatching()
        let rows = verticalSizeClass == .compact ? 3 : 6
        return stride(from: 0, to: items.count, by: rows).map { start in
            ProfileContinueColumn(items: Array(items[start..<min(start + rows, items.count)]))
        }
    }

    private var savedColumns: [ProfileSavedColumn] {
        let items = libraryStore.library.list
        let rows = verticalSizeClass == .compact ? 3 : 6
        return stride(from: 0, to: items.count, by: rows).map { start in
            ProfileSavedColumn(items: Array(items[start..<min(start + rows, items.count)]))
        }
    }

    private func sectionHeading(_ collection: ProfileCollection, count: Int, availableWidth: CGFloat = 0) -> some View {
        HStack(alignment: .center, spacing: 8) {
            // Balance the paging control so the selector stays centered in every collection.
            Color.clear
                .frame(width: 44, height: 44)
                .accessibilityHidden(true)
            if #available(iOS 26.0, *) {
                profileCollectionPicker
                    .padding(4)
                    .glassEffect(.regular, in: Capsule())
            } else {
                profileCollectionPicker
            }
            Color.clear
                .frame(width: 44, height: 44)
                .overlay {
            if collection == .continueWatching {
                let columns = continueColumns
                if columns.count > 1 {
                    let current = columns.firstIndex(where: { $0.id == continueScrollID }) ?? 0
                    let atEnd = current == columns.count - 1
                    Button {
                        let next = atEnd ? 0 : min(current + 1, columns.count - 1)
                        withAnimation(reduceMotion ? nil : .smooth(duration: 0.35)) {
                            continueScrollID = columns[next].id
                        }
                    } label: {
                        Image(systemName: atEnd ? "arrow.left" : "arrow.right")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(MelloTVTheme.textSecondary)
                            .frame(width: 44, height: 44)
                            .background(.thinMaterial, in: Circle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(atEnd ? "Back to first titles" : "Scroll to more titles")
                }
            } else if collection == .myList {
                let columns = savedColumns
                if columns.count > 1 {
                    let current = columns.firstIndex(where: { $0.id == savedScrollID }) ?? 0
                    let atEnd = current == columns.count - 1
                    Button {
                        let next = atEnd ? 0 : min(current + 1, columns.count - 1)
                        withAnimation(reduceMotion ? nil : .smooth(duration: 0.35)) {
                            savedScrollID = columns[next].id
                        }
                    } label: {
                        Image(systemName: atEnd ? "arrow.left" : "arrow.right")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(MelloTVTheme.textSecondary)
                            .frame(width: 44, height: 44)
                            .background(.thinMaterial, in: Circle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(atEnd ? "Back to first saved titles" : "Scroll to more saved titles")
                }
            }
                }
        }
        .frame(maxWidth: .infinity, minHeight: 44, alignment: .center)
        .padding(.horizontal, 16)
    }

    private var profileCollectionPicker: some View {
        Picker("Library collection", selection: $selectedCollection) {
            Text("Continue").tag(ProfileCollection.continueWatching)
            Text("My List").tag(ProfileCollection.myList)
            Text("History").tag(ProfileCollection.history)
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .frame(maxWidth: .infinity, minHeight: 36)
        .accessibilityLabel("Library collection")
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
        .background {
            HeroSatinBackdrop(strength: 0.18, reactsToMotion: false).ignoresSafeArea()
        }
        .navigationTitle(collection.rawValue)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar(.visible, for: .navigationBar)
    }

    private var appearanceSettingsSection: some View {
        VStack(spacing: 4) {
            Menu {
                Picker("Appearance", selection: $appearanceMode) {
                    ForEach(AppearanceMode.allCases) { mode in
                        Label(mode.rawValue, image: mode.iconName)
                            .tag(mode)
                    }
                }
            } label: {
                HStack(spacing: 10) {
                    PhosphorIcon(appearanceMode.iconName, size: 17)
                    Text("Appearance")
                        .font(.subheadline)
                    Spacer(minLength: 8)
                    Text(appearanceMode.rawValue)
                        .font(.subheadline.weight(.medium))
                    PhosphorIcon("caret-up", size: 10, weight: .bold)
                }
                .foregroundStyle(MelloTVTheme.textSecondary)
                .frame(minHeight: 40)
                .padding(.vertical, 4)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Appearance")
            .accessibilityValue(appearanceMode.rawValue)

            Button {
                openURL(AppLinks.buyMeACoffeeURL)
            } label: {
                HStack(spacing: 10) {
                    Image(systemName: "cup.and.saucer.fill")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(Color(red: 1.0, green: 0.84, blue: 0.1))
                    Text("Buy Me a Coffee")
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(MelloTVTheme.text)
                    Spacer(minLength: 8)
                    Text("Support Mello TV")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(Color(red: 1.0, green: 0.84, blue: 0.1))
                    PhosphorIcon("caret-right", size: 11, weight: .bold)
                        .foregroundStyle(MelloTVTheme.muted)
                }
                .frame(minHeight: 40)
                .padding(.vertical, 4)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Buy Me a Coffee, Support Mello TV")
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
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 240), spacing: 12)],
                          alignment: .leading, spacing: 24) {
                    ForEach(items, id: \.profileID) { item in progressCard(item, profile: profile) }
                }
            }
        case .history:
            if libraryStore.watchHistory.isEmpty {
                emptyState(collection)
            } else {
                historyJournal(profile)
            }
        }
    }

    private func progressCard(_ item: WatchProgress, profile: Profile) -> some View {
        ContinueWatchingCard(
            item: item,
            isPreparingPlayback: resolvingItem == item.profileID,
            isPlaybackDisabled: resolvingItem != nil,
            onPlay: { resume(item, profile: profile) },
            onInfo: { selectedTitleForDetails = item.title }
        )
    }

    private var historyDays: [ProfileHistoryDay] {
        let calendar = Calendar.autoupdatingCurrent
        var grouped: [Date: [WatchProgress]] = [:]
        var undated: [WatchProgress] = []
        let sorted = libraryStore.watchHistory.sorted {
            let left = $0.updatedAt.isFinite ? $0.updatedAt : 0
            let right = $1.updatedAt.isFinite ? $1.updatedAt : 0
            return left > right
        }
        for item in sorted {
            guard item.updatedAt.isFinite, item.updatedAt > 0, item.updatedAt < 253402300799 else {
                undated.append(item)
                continue
            }
            let day = calendar.startOfDay(for: Date(timeIntervalSince1970: item.updatedAt))
            grouped[day, default: []].append(item)
        }
        var result = grouped.keys.sorted(by: >).map { day in
            ProfileHistoryDay(date: day, items: grouped[day] ?? [])
        }
        if !undated.isEmpty { result.append(ProfileHistoryDay(date: nil, items: undated)) }
        return result
    }

    private func historyJournal(_ profile: Profile) -> some View {
        LazyVStack(alignment: .leading, spacing: 28) {
            ForEach(historyDays) { day in
                VStack(alignment: .leading, spacing: 12) {
                    HStack(alignment: .center, spacing: 12) {
                        if let date = day.date {
                            Text(date, format: .dateTime.day())
                                .font(.system(.largeTitle, design: .rounded, weight: .light))
                                .monospacedDigit()
                                .foregroundStyle(MelloTVTheme.text)
                            VStack(alignment: .leading, spacing: 3) {
                                Text(historyDayTitle(date))
                                    .font(.subheadline.weight(.semibold))
                                    .foregroundStyle(MelloTVTheme.text)
                                Text(date, format: .dateTime.month(.wide).year())
                                    .font(.caption)
                                    .foregroundStyle(MelloTVTheme.textSecondary)
                            }
                        } else {
                            Text("Earlier viewing")
                                .font(.subheadline.weight(.semibold))
                        }
                        Spacer(minLength: 0)
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityAddTraits(.isHeader)

                    VStack(spacing: 0) {
                        ForEach(day.items, id: \.profileID) { item in
                            historyRow(item, profile: profile)
                                .padding(.leading, 22)
                                .background(alignment: .leading) {
                                    Rectangle()
                                        .fill(MelloTVTheme.text.opacity(0.10))
                                        .frame(width: 1)
                                        .padding(.leading, 3)
                                }
                                .overlay(alignment: .leading) {
                                    Circle()
                                        .fill(item.completed ? ProfileColor.color(for: profile.color) : MelloTVTheme.textSecondary)
                                        .frame(width: 7, height: 7)
                                        .accessibilityHidden(true)
                                }
                        }
                    }
                }
            }
        }
    }

    private func historyDayTitle(_ date: Date) -> String {
        let calendar = Calendar.autoupdatingCurrent
        if calendar.isDateInToday(date) { return "Today" }
        if calendar.isDateInYesterday(date) { return "Yesterday" }
        return date.formatted(.dateTime.weekday(.wide))
    }

    private func historyRow(_ item: WatchProgress, profile: Profile) -> some View {
        Group {
            if dynamicTypeSize.isAccessibilitySize {
                VStack(alignment: .leading, spacing: 10) {
                    playbackArtwork(item, profile: profile)
                    historyTitle(item)
                }
            } else {
                HStack(alignment: .center, spacing: 12) {
                    playbackArtwork(item, profile: profile)
                        .frame(width: verticalSizeClass == .compact ? 140 : 96)
                    historyTitle(item)
                }
            }
        }
        .padding(.vertical, 12)
        .overlay(alignment: .bottom) {
            Rectangle().fill(MelloTVTheme.text.opacity(0.06)).frame(height: 0.5)
        }
    }

    private func historyTitle(_ item: WatchProgress) -> some View {
        Button { selectedTitleForDetails = item.title } label: {
            VStack(alignment: .leading, spacing: 5) {
                Text(item.title.title)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(MelloTVTheme.text)
                    .lineLimit(2)
                if item.title.type == .tv {
                    Text("S\(item.season), E\(item.episode)" + (item.episodeTitle.isEmpty ? "" : " · \(item.episodeTitle)"))
                        .font(.caption)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .lineLimit(2)
                }
                HStack(spacing: 4) {
                    if item.completed {
                        Image(systemName: "checkmark").font(.system(size: 9, weight: .bold))
                        Text("Watched")
                    } else if let fraction = item.profileFraction {
                        let minutes = ceil(item.duration * (1 - fraction) / 60)
                        if minutes.isFinite && minutes < Double(Int.max) {
                            Text("\(max(1, Int(minutes)))m remaining")
                        }
                    } else {
                        Text("In progress")
                    }
                }
                .font(.caption2.weight(.medium))
                .foregroundStyle(MelloTVTheme.textSecondary)
            }
            .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
            .multilineTextAlignment(.leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityHint("Shows title details")
    }

    private func playbackArtwork(_ item: WatchProgress, profile: Profile) -> some View {
        Button { resume(item, profile: profile) } label: {
            ProfileArtwork(title: item.title, isPoster: false)
                .overlay { playbackGlyph(item) }
                .overlay(alignment: .bottom) { progressBar(item) }
                .clipShape(.rect(cornerRadius: 6))
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(MelloTVTheme.subtleBorder, lineWidth: 0.5)
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
                        .foregroundStyle(MelloTVTheme.text)
                        .lineLimit(1)

                    Text(item.profileSummary)
                        .font(.caption)
                        .foregroundStyle(MelloTVTheme.muted)
                        .lineLimit(1)

                    if showsDate, item.updatedAt.isFinite, item.updatedAt > 0 {
                        Text(Date(timeIntervalSince1970: item.updatedAt), style: .date)
                            .font(.caption2)
                            .foregroundStyle(MelloTVTheme.mutedSubtle)
                    }
                }
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)

                PhosphorIcon("caret-right", size: 12, weight: .bold)
                    .foregroundStyle(MelloTVTheme.muted.opacity(0.6))
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
                .clipShape(.rect(cornerRadius: 18))
                .overlay(
                    RoundedRectangle(cornerRadius: 18)
                        .stroke(MelloTVTheme.subtleBorder, lineWidth: 0.5)
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.96))
        .accessibilityLabel("\(title.title), \(title.type == .movie ? "Movie" : "Series"), \(title.year)")
        .accessibilityHint("Shows title details and My List controls")
        .overlay(alignment: .bottomTrailing) {
            Button { selectedTitleForDetails = title } label: {
                CardDetailsGlyph()
            }
            .buttonStyle(.plain)
            .padding(.trailing, 12)
            .padding(.bottom, 2)
            .accessibilityLabel("Details for \(title.title)")
        }
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
                        .fill(item.completed ? MelloTVTheme.muted : MelloTVTheme.accent)
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
                    .foregroundStyle(MelloTVTheme.muted)
                    .padding(.bottom, 8)
                    .accessibilityHidden(true)
            }
            Text(collection.emptyTitle)
                .font(.headline)
                .foregroundStyle(MelloTVTheme.text)
            Text(collection.emptyMessage)
                .font(.subheadline)
                .foregroundStyle(MelloTVTheme.muted)
                .fixedSize(horizontal: false, vertical: true)
            if let onBrowseTitles, !compact || collection == .myList {
                Button("Browse titles", action: onBrowseTitles)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(MelloTVTheme.background)
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
                            image.resizable()
                                .aspectRatio(contentMode: isPoster ? .fit : .fill)
                        } else {
                            MelloTVTheme.cardBackground
                                .overlay {
                                    PhosphorIcon(title.type == .movie ? "film-slate" : "television-simple", size: 24)
                                        .foregroundStyle(MelloTVTheme.muted)
                                }
                        }
                    }
                    .frame(width: geometry.size.width, height: geometry.size.height)
                    .background(MelloTVTheme.secondaryBackground)
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

private extension VerticalAlignment {
    enum ProfileIdentityCenter: AlignmentID {
        static func defaultValue(in context: ViewDimensions) -> CGFloat { context[VerticalAlignment.center] }
    }
    static let profileIdentityCenter = VerticalAlignment(ProfileIdentityCenter.self)
}

private struct ProfileContinueColumn: Identifiable {
    let items: [WatchProgress]
    var id: ProfileProgressID { items[0].profileID }
}

private struct ProfileSavedColumn: Identifiable {
    let items: [MediaTitle]
    var id: String { items[0].id }
}

/// Layout follows content count: one centered row for small collections,
/// balanced rows for larger ones, and full-width pages for overflow.
private struct ProfileAdaptivePosterShelf<Item, ID: Hashable, Content: View>: View {
    let items: [Item]
    let id: KeyPath<Item, ID>
    @Binding var scrollID: ID?
    @ViewBuilder var poster: (Item, CGFloat) -> Content
    @Environment(\.verticalSizeClass) private var verticalSizeClass
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    private var capacity: Int { verticalSizeClass == .compact ? 3 : 6 }
    private var pages: [ProfilePosterPage<Item, ID>] {
        stride(from: 0, to: items.count, by: capacity).map { start in
            ProfilePosterPage(id: items[start][keyPath: id], items: Array(items[start..<min(start + capacity, items.count)]))
        }
    }

    var body: some View {
        GeometryReader { geometry in
            ScrollView(.horizontal) {
                LazyHStack(spacing: 0) {
                    ForEach(pages) { page in
                        let rows = verticalSizeClass == .compact || page.items.count <= 3 ? 1 : 2
                        let columns = (page.items.count + rows - 1) / rows
                        let widthLimit = (geometry.size.width - 48 - CGFloat(max(0, columns - 1)) * 14) / CGFloat(max(1, columns))
                        let heightLimit = (geometry.size.height - 10 - CGFloat(rows - 1) * 14) / CGFloat(rows) * 2 / 3
                        let width = max(1, min(dynamicTypeSize.isAccessibilitySize ? 200 : 160, widthLimit, heightLimit))
                        VStack(spacing: 14) {
                            ForEach(0..<rows, id: \.self) { row in
                                let start = row * columns
                                let end = min(start + columns, page.items.count)
                                HStack(spacing: 14) {
                                    ForEach(Array(page.items[start..<end]), id: id) { item in
                                        poster(item, width)
                                    }
                                }
                                .frame(maxWidth: .infinity, alignment: .center)
                            }
                        }
                        .frame(width: max(0, geometry.size.width - 48), height: max(0, geometry.size.height - 10), alignment: .center)
                        .padding(.horizontal, 24)
                    }
                }
                .scrollTargetLayout()
            }
            .scrollTargetBehavior(.paging)
            .scrollPosition(id: $scrollID, anchor: .leading)
            .onChange(of: pages.map(\.id)) { _, ids in
                if let scrollID, !ids.contains(scrollID) { self.scrollID = nil }
            }
        }
    }
}

private struct ProfilePosterPage<Item, ID: Hashable>: Identifiable {
    let id: ID
    let items: [Item]
}

private struct ProfileHistoryDay: Identifiable {
    let date: Date?
    let items: [WatchProgress]
    var id: String { date.map { String($0.timeIntervalSince1970) } ?? "undated" }
}
