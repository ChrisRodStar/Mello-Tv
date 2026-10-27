import SwiftUI

struct OnboardingView: View {
    @Bindable var profileStore: ProfileStore
    let profile: Profile
    var onCompleted: (() -> Void)? = nil

    @Environment(\.dismiss) private var dismiss
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @ScaledMetric(relativeTo: .caption) private var posterMinimum: CGFloat = 112

    private enum Step { case genres, titles, saving }

    @State private var currentStep: Step = .genres
    @State private var format = "both"
    @State private var selectedGenres: Set<String> = []
    @State private var selectedLikedTitles: [LikedTitle] = []
    @State private var candidateTitles: [MediaTitle] = []
    @State private var genreArtworks: [String: GenreArtwork] = [:]
    @State private var isLoading = false
    @State private var errorMessage: String?
    @State private var hasInitialized = false
    @State private var showFavorites = false
    @State private var operationTask: Task<Void, Never>?
    private let api = MelloTVAPI(baseURL: ServerAddress.baseURL)

    private var isEditing: Bool { profile.preferences.completed }
    private var selectedCount: Int { currentStep == .genres ? selectedGenres.count : selectedLikedTitles.count }

    var body: some View {
        NavigationStack {
            GeometryReader { geometry in
                let contentWidth = max(1, min(960, geometry.size.width - 32))
                let inlineAction = geometry.size.height < 260 || dynamicTypeSize.isAccessibilitySize
                let short = geometry.size.height < 500 && !dynamicTypeSize.isAccessibilitySize
                ScrollView {
                    VStack(alignment: .leading, spacing: short ? 12 : 16) {
                        if currentStep == .saving {
                            savingContent
                        } else {
                            introduction(short: short)
                            if currentStep == .genres {
                                genreContent(width: contentWidth, height: geometry.size.height, short: short)
                            } else {
                                titleContent(width: contentWidth)
                            }
                            if inlineAction { actionBar(isInline: true) }
                        }
                    }
                    .frame(maxWidth: 960, alignment: .leading)
                    .frame(maxWidth: .infinity)
                    .padding(.horizontal, 16)
                    .padding(.top, 8)
                    .padding(.bottom, 12)
                    .frame(minHeight: inlineAction ? nil : max(0, geometry.size.height - 92), alignment: .top)
                }
                .scrollBounceBehavior(.basedOnSize)
                .safeAreaInset(edge: .bottom, spacing: 0) {
                    if currentStep != .saving && !inlineAction { actionBar(isInline: false) }
                }
            }
            .background(MelloTVTheme.background.ignoresSafeArea())
            .foregroundStyle(MelloTVTheme.text)
            .toolbar(.hidden, for: .navigationBar)
            .task {
                // Rotation and returning from another app must not reset this draft.
                guard !hasInitialized else { return }
                hasInitialized = true
                format = profile.preferences.format.isEmpty ? "both" : profile.preferences.format
                selectedGenres = Set(profile.preferences.genres.filter { $0 != "family" })
                selectedLikedTitles = profile.preferences.likedTitles
                do { genreArtworks = try await api.genreArtwork() }
                catch { /* Artwork is optional; every genre remains selectable. */ }
            }
            .onDisappear { operationTask?.cancel() }
        }
        .tint(MelloTVTheme.text)
        .sheet(isPresented: $showFavorites) { favoriteSelections }
    }

    private func introduction(short: Bool) -> some View {
        HStack(alignment: .top, spacing: 8) {
            if currentStep == .titles {
                Button {
                    errorMessage = nil
                    currentStep = .genres
                } label: {
                    PhosphorIcon("caret-left", size: 18, weight: .bold).touchTarget()
                }
                .disabled(isLoading)
                .accessibilityLabel("Back to genres")
            }
            VStack(alignment: .leading, spacing: 6) {
                Text(currentStep == .genres ? "What do you enjoy?" : "Pick your favorites.")
                    .font(short ? .title2.weight(.bold) : .title.weight(.bold))
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                if !short {
                    Text(currentStep == .genres ? "Choose at least 3 genres for your recommendations."
                         : "Choose at least 3 movies or series you enjoy.")
                        .font(.subheadline)
                        .foregroundStyle(MelloTVTheme.muted)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if isEditing {
                Button { dismiss() } label: {
                    PhosphorIcon("x", size: 18, weight: .bold).touchTarget()
                }
                .disabled(isLoading)
                .accessibilityLabel("Close preferences without saving")
            }
        }
        .buttonStyle(.plain)
    }

    private func genreContent(width: CGFloat, height: CGFloat, short: Bool) -> some View {
        let wide = short && width >= 640
        let columns = dynamicTypeSize.isAccessibilitySize ? 1 : width >= 640 ? 4 : 2
        let rows = (GenreOption.all.count + columns - 1) / columns
        let artworkHeight: CGFloat? = dynamicTypeSize.isAccessibilitySize ? nil
            : min(short ? 112 : 144, max(44, (height - (short ? 200 : 220) - CGFloat(rows - 1) * 12) / CGFloat(rows) - (short ? 40 : 28)))
        let layout = wide ? AnyLayout(HStackLayout(alignment: .top, spacing: 20))
                          : AnyLayout(VStackLayout(alignment: .leading, spacing: 16))
        return layout {
            formatSection.frame(maxWidth: wide ? 152 : .infinity, alignment: .leading)
            genreSection(columns: columns, artworkHeight: artworkHeight)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .disabled(isLoading)
    }

    private var formatSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: dynamicTypeSize.isAccessibilitySize ? 220 : 110), spacing: 8)], spacing: 8) {
                ForEach(PreferenceFormat.allCases) { option in
                    Button { format = option.rawValue } label: {
                        HStack(spacing: 6) {
                            PhosphorIcon(option.symbol, size: 18)
                                .accessibilityHidden(true)
                            Text(option.label).font(.caption.weight(.semibold))
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .padding(.horizontal, 8)
                        .foregroundStyle(format == option.rawValue ? MelloTVTheme.background : MelloTVTheme.textSecondary)
                        .background(format == option.rawValue ? MelloTVTheme.text : MelloTVTheme.cardBackground,
                                    in: .rect(cornerRadius: 8))
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(CardScaleButtonStyle(scale: 0.98))
                    .accessibilityAddTraits(format == option.rawValue ? .isSelected : [])
                }
            }
        }
    }

    private func genreSection(columns: Int, artworkHeight: CGFloat?) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionTitle("Genres")
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 14), count: columns), alignment: .leading, spacing: 12) {
                ForEach(GenreOption.all) { genre in
                    PreferenceSelectionTile(name: genre.name, subtitle: nil,
                                            imageURL: genreArtworks[genre.id]?.imageUrl,
                                            isPoster: false, isSelected: selectedGenres.contains(genre.id), imageHeight: artworkHeight) {
                        if !selectedGenres.insert(genre.id).inserted { selectedGenres.remove(genre.id) }
                    }
                }
            }
        }
    }

    private func titleContent(width: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            if candidateTitles.isEmpty {
                VStack(alignment: .leading, spacing: 12) {
                    sectionTitle("No titles available for these genres")
                    Text("Go back to try other genres or reload the available titles. Your selections are kept.")
                        .font(.subheadline).foregroundStyle(MelloTVTheme.muted)
                    Button("Reload titles", action: loadTitles)
                        .font(.subheadline.weight(.semibold))
                        .touchTarget()
                }
            } else {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: min(posterMinimum, width)), spacing: 12)],
                          alignment: .leading, spacing: 24) {
                    ForEach(candidateTitles) { title in
                        PreferenceSelectionTile(name: title.title,
                                                subtitle: [title.type == .movie ? "Movie" : "Series", title.year]
                                                    .filter { !$0.isEmpty }.joined(separator: " · "),
                                                imageURL: title.posterURL, isPoster: true,
                                                isSelected: isFavorite(title)) {
                            toggleFavorite(title)
                        }
                    }
                }
            }
        }
        .disabled(isLoading)
    }

    private func actionBar(isInline: Bool) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            if let errorMessage {
                Text(errorMessage)
                    .font(.subheadline)
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 24) {
                    selectionControl.frame(minWidth: 140, maxWidth: .infinity, alignment: .leading)
                    primaryAction.frame(minWidth: 164, maxWidth: 280)
                }
                VStack(alignment: .leading, spacing: 12) {
                    selectionControl
                    primaryAction
                }
            }
        }
        .frame(maxWidth: 960, alignment: .leading)
        .frame(maxWidth: .infinity)
        .padding(.horizontal, isInline ? 0 : 16)
        .padding(.top, 12)
        .padding(.bottom, 8)
        .background(MelloTVTheme.background)
        .overlay(alignment: .top) { Rectangle().fill(MelloTVTheme.subtleBorder).frame(height: 1) }
    }

    @ViewBuilder
    private var selectionControl: some View {
        if currentStep == .titles {
            Button { showFavorites = true } label: {
                HStack(spacing: 6) {
                    selectionSummary
                    PhosphorIcon("caret-up", size: 12, weight: .bold)
                }
                .frame(minHeight: 44)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityHint("Review and remove selected favorites")
        } else {
            selectionSummary
        }
    }

    private var favoriteSelections: some View {
        NavigationStack {
            List {
                ForEach(selectedLikedTitles, id: \.preferenceIdentity) { liked in
                    HStack(spacing: 12) {
                        Text(liked.title).font(.subheadline)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        Button { removeFavorite(liked) } label: {
                            PhosphorIcon("minus-circle", size: 20, weight: .fill).touchTarget()
                        }
                        .buttonStyle(.borderless)
                        .accessibilityLabel("Remove \(liked.title) from favorites")
                    }
                    .listRowBackground(MelloTVTheme.cardBackground)
                }
            }
            .scrollContentBackground(.hidden)
            .background(MelloTVTheme.background)
            .navigationTitle("Selected favorites")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { showFavorites = false }
                }
            }
            .overlay {
                if selectedLikedTitles.isEmpty {
                    Text("No favorites selected").foregroundStyle(MelloTVTheme.muted)
                }
            }
        }
    }

    private var selectionSummary: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text("\(selectedCount) \(currentStep == .genres ? "genres" : "favorites") selected")
                .font(.subheadline.weight(.semibold))
            Text(selectedCount < 3 ? "Choose \(3 - selectedCount) more to continue" : currentStep == .genres ? "Next, pick a few favorites" : "Ready to save")
                .font(.caption)
                .foregroundStyle(MelloTVTheme.muted)
        }
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityElement(children: .combine)
    }

    private var primaryAction: some View {
        Button(action: currentStep == .genres ? loadTitles : savePreferences) {
            HStack(spacing: 10) {
                if isLoading { ProgressView().tint(MelloTVTheme.background) }
                Text(isLoading ? "Loading titles…" : currentStep == .genres ? "Continue" : isEditing ? "Save preferences" : "Find my recommendations")
                    .font(.subheadline.weight(.semibold))
                    .fixedSize(horizontal: false, vertical: true)
            }
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity, minHeight: 44)
            .padding(.horizontal, 16)
            .padding(.vertical, 4)
            .foregroundStyle(MelloTVTheme.background)
            .background(MelloTVTheme.text, in: .rect(cornerRadius: 6))
            .contentShape(Rectangle())
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.98))
        .disabled(selectedCount < 3 || isLoading)
        .opacity(selectedCount < 3 ? 0.45 : 1)
    }

    private var savingContent: some View {
        VStack(alignment: .leading, spacing: 20) {
            ProgressView().tint(MelloTVTheme.accent).accessibilityLabel("Saving preferences")
            Text("Saving your preferences…")
                .font(.largeTitle.weight(.bold))
                .fixedSize(horizontal: false, vertical: true)
            Text("Your home screen uses these choices for recommendations.")
                .font(.subheadline)
                .foregroundStyle(MelloTVTheme.muted)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, 28)
    }

    private func sectionTitle(_ text: String) -> some View {
        Text(text).font(.title3.weight(.bold)).accessibilityAddTraits(.isHeader)
    }

    private func isFavorite(_ title: MediaTitle) -> Bool {
        selectedLikedTitles.contains { $0.id == title.tmdbID && $0.type == title.type }
    }

    private func toggleFavorite(_ title: MediaTitle) {
        if isFavorite(title) {
            selectedLikedTitles.removeAll { $0.id == title.tmdbID && $0.type == title.type }
        } else {
            selectedLikedTitles.append(LikedTitle(id: title.tmdbID, type: title.type, title: title.title))
        }
    }

    private func removeFavorite(_ title: LikedTitle) {
        selectedLikedTitles.removeAll { $0.id == title.id && $0.type == title.type }
    }

    private func loadTitles() {
        guard !isLoading, selectedGenres.count >= 3 else { return }
        isLoading = true
        errorMessage = nil
        let preferences = ProfilePreferences(genres: selectedGenres.sorted(), format: format, likedTitles: [], completed: false)
        operationTask = Task { @MainActor in
            defer { isLoading = false; operationTask = nil }
            do {
                let titles = try await api.onboardingTitles(preferences: preferences, profileID: profile.id)
                try Task.checkCancellation()
                candidateTitles = titles
                currentStep = .titles
            } catch {
                if !Task.isCancelled { errorMessage = error.localizedDescription }
            }
        }
    }

    private func savePreferences() {
        guard !isLoading, selectedLikedTitles.count >= 3 else { return }
        isLoading = true
        errorMessage = nil
        currentStep = .saving
        let preferences = ProfilePreferences(genres: selectedGenres.sorted(), format: format,
                                             likedTitles: selectedLikedTitles, completed: true)
        operationTask = Task { @MainActor in
            defer { isLoading = false; operationTask = nil }
            do {
                try await profileStore.completeOnboarding(with: preferences)
                try Task.checkCancellation()
                onCompleted?()
            } catch {
                if !Task.isCancelled {
                    errorMessage = error.localizedDescription
                    currentStep = .titles
                }
            }
        }
    }
}

private enum PreferenceFormat: String, CaseIterable, Identifiable {
    case both, movie, tv
    var id: Self { self }
    var label: String {
        switch self {
        case .both: "Movies & TV"
        case .movie: "Movies"
        case .tv: "TV shows"
        }
    }
    var symbol: String {
        switch self {
        case .both: "stack"
        case .movie: "film-slate"
        case .tv: "television-simple"
        }
    }
}

private struct PreferenceSelectionTile: View {
    let name: String
    let subtitle: String?
    let imageURL: URL?
    let isPoster: Bool
    let isSelected: Bool
    var imageHeight: CGFloat? = nil
    var action: () -> Void

    @ViewBuilder
    private var artworkBounds: some View {
        if let imageHeight {
            Color.clear.frame(height: imageHeight)
        } else {
            Color.clear.aspectRatio(isPoster ? 2.0 / 3 : 16.0 / 9, contentMode: .fit)
        }
    }

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 8) {
                artworkBounds
                    .overlay {
                        GeometryReader { geometry in
                            AsyncImage(url: imageURL) { phase in
                                if case .success(let image) = phase {
                                    if isPoster {
                                        image.resizable().scaledToFit()
                                    } else {
                                        image.resizable().scaledToFill()
                                    }
                                } else {
                                    MelloTVTheme.secondaryBackground
                                        .overlay {
                                            PhosphorIcon(isPoster ? "film-slate" : "stack", size: 24)
                                                .foregroundStyle(MelloTVTheme.muted)
                                        }
                                }
                            }
                            .frame(width: geometry.size.width, height: geometry.size.height)
                            .clipped()
                        }
                    }
                    .clipShape(.rect(cornerRadius: 8))
                    .overlay {
                        RoundedRectangle(cornerRadius: 8)
                            .strokeBorder(isSelected ? MelloTVTheme.accent : MelloTVTheme.subtleBorder,
                                          lineWidth: isSelected ? 2 : 0.5)
                    }
                    .overlay(alignment: .topTrailing) {
                        PhosphorIcon(isSelected ? "check" : "circle", size: 12, weight: isSelected ? .bold : .regular)
                            .foregroundStyle(isSelected ? .white : MelloTVTheme.textSecondary)
                            .padding(5)
                            .background(isSelected ? MelloTVTheme.accent : .black.opacity(0.65), in: Circle())
                            .padding(8)
                    }
                    .accessibilityHidden(true)
                Text(name)
                    .font(isPoster ? .caption.weight(.semibold) : .subheadline.weight(.semibold))
                    .lineLimit(imageHeight == nil ? nil : 2)
                    .foregroundStyle(MelloTVTheme.text)
                    .fixedSize(horizontal: false, vertical: true)
                if let subtitle {
                    Text(subtitle).font(.caption).foregroundStyle(MelloTVTheme.muted)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .multilineTextAlignment(.leading)
            .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.98))
        .accessibilityLabel([name, subtitle].compactMap { $0 }.joined(separator: ", "))
        .accessibilityValue(isSelected ? "Selected" : "Not selected")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
        .accessibilityHint(isSelected ? "Double tap to remove" : "Double tap to select")
    }
}

private extension LikedTitle {
    var preferenceIdentity: String { "\(type.rawValue)-\(id)" }
}
