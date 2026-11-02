import MelloKit
import SwiftUI

struct RecommendationChoicesView: View {
    let profileID: String
    let libraryStore: LibraryStore

    @State private var store: RecommendationChoicesStore
    @State private var selectedCategory: RecommendationChoice = .favorites
    @State private var isEditing = false
    @State private var selectedIDs: Set<String> = []
    @State private var selectedTitle: MediaTitle?
    @State private var pendingPlayback: PlaybackRequest?
    @State private var playbackRequest: PlaybackRequest?

    init(profileID: String, libraryStore: LibraryStore) {
        self.profileID = profileID
        self.libraryStore = libraryStore
        _store = State(initialValue: RecommendationChoicesStore(profileID: profileID))
    }

    private var activeCategory: RecommendationChoice? {
        store.categories.contains(selectedCategory) ? selectedCategory : store.categories.first
    }

    private var titles: [MediaTitle] {
        activeCategory.map { store.titles(for: $0) } ?? []
    }

    var body: some View {
        NavigationStack {
            Group {
                if let category = activeCategory {
                    gallery(category: category)
                } else if store.choices != nil {
                    ContentUnavailableView {
                        Text("No saved choices")
                    } description: {
                        Text("Choices you make on a movie or series appear here.")
                    }
                } else if let error = store.error {
                    ContentUnavailableView {
                        Text("Couldn’t load choices")
                    } description: {
                        Text(error)
                    } actions: {
                        Button("Try again") { Task { await store.load() } }
                    }
                } else {
                    ProgressView("Loading choices")
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background { CatalogPageSurface().ignoresSafeArea() }
            .toolbar(.hidden, for: .navigationBar)
            .task { await store.load() }
            .onChange(of: activeCategory) { _, category in
                selectedIDs.removeAll()
                if let category { selectedCategory = category }
            }
            .onChange(of: titles.map(\.id)) { _, ids in selectedIDs.formIntersection(ids) }
            .onReceive(NotificationCenter.default.publisher(for: .melloTVRecommendationsChanged)) {
                note in
                guard note.userInfo?["profileID"] as? String == profileID, !store.isSaving else {
                    return
                }
                Task { await store.load() }
            }
        }
        .presentationDetents([.large])
        .presentationDragIndicator(.visible)
        .presentationCornerRadius(28)
        .interactiveDismissDisabled(store.isSaving)
        .sheet(item: $selectedTitle, onDismiss: presentPendingPlayback) { title in
            TitleDetailSheet(title: title, profileID: profileID, libraryStore: libraryStore) {
                request in
                pendingPlayback = request
                selectedTitle = nil
            }
        }
        .fullScreenCover(item: $playbackRequest) { request in
            PlayerView(
                request: request,
                onDismiss: {
                    playbackRequest = nil
                    Task { await libraryStore.refresh(for: profileID) }
                })
        }
    }

    // MARK: - Browsing

    private func gallery(category: RecommendationChoice) -> some View {
        VStack(spacing: 0) {
            RecommendationCategoryPicker(
                categories: store.categories,
                selected: category,
                counts: Dictionary(
                    uniqueKeysWithValues: store.categories.map { ($0, store.titles(for: $0).count) }
                ),
                onSelect: {
                    selectedCategory = $0
                    selectedIDs.removeAll()
                }
            )
            .disabled(store.isSaving)

            ScrollView {
                RecommendationPosterGrid(
                    titles: titles,
                    isEditing: isEditing,
                    selectedIDs: selectedIDs,
                    onTap: { title in
                        if isEditing {
                            if !selectedIDs.insert(title.id).inserted {
                                selectedIDs.remove(title.id)
                            }
                        } else {
                            selectedTitle = title
                        }
                    },
                    onSelect: { title in
                        isEditing = true
                        selectedIDs.insert(title.id)
                    }
                )
                .padding(.horizontal, 20)
                .padding(.top, 20)
                .padding(.bottom, 24)
            }
            .id(category)
            .disabled(store.isSaving)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if isEditing || store.error != nil {
                VStack(alignment: .leading, spacing: 8) {
                    if let error = store.error {
                        Text(error)
                            .font(.footnote)
                            .foregroundStyle(MelloTVTheme.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    if isEditing { selectionControls }
                }
                .padding(.horizontal, 20)
                .padding(.vertical, 10)
                .background(MelloTVTheme.cardBackground)
            }
        }
    }

    // MARK: - Editing

    private var selectionControls: some View {
        HStack(spacing: 16) {
            Button("Cancel") {
                isEditing = false
                selectedIDs.removeAll()
            }
            Button(selectedIDs.count == titles.count ? "Deselect all" : "Select all") {
                selectedIDs = selectedIDs.count == titles.count ? [] : Set(titles.map(\.id))
            }
            Spacer(minLength: 12)
            if store.isSaving {
                ProgressView("Saving")
            } else {
                Button("Remove (\(selectedIDs.count))") { Task { await removeSelection() } }
                    .disabled(selectedIDs.isEmpty)
            }
        }
        .font(.subheadline.weight(.medium))
        .frame(minHeight: 44)
        .disabled(store.isSaving)
    }

    private func removeSelection() async {
        guard let category = activeCategory else { return }
        let removed = await store.remove(ids: selectedIDs, from: category)
        selectedIDs.subtract(removed)
        if store.error == nil { isEditing = false }
    }

    private func presentPendingPlayback() {
        guard let request = pendingPlayback else { return }
        pendingPlayback = nil
        playbackRequest = request
    }
}
