import MelloKit
import SwiftUI

/// A pushed collection page shares the same library and playback flow as browsing.
struct LibraryCollectionPage: View {
    let collection: LibraryCollection
    let profileID: String
    let libraryStore: LibraryStore
    var onBrowseTitles: () -> Void

    @State private var selectedTitle: MediaTitle?
    @State private var playbackRequest: PlaybackRequest?
    @State private var resolvingItem: LibraryProgressID?
    @State private var resumeTask: Task<Void, Never>?

    private var isEmpty: Bool {
        switch collection {
        case .continueWatching: libraryStore.continueWatching().isEmpty
        case .myList: libraryStore.library.list.isEmpty
        case .history: libraryStore.watchHistory.isEmpty
        }
    }

    var body: some View {
        Group {
            if libraryStore.isLoading && isEmpty {
                ProgressView("Loading your library")
                    .frame(maxWidth: .infinity, minHeight: 200)
            } else if let message = libraryStore.errorMessage, isEmpty {
                ContentUnavailableView {
                    Label("Couldn’t load your library", systemImage: "wifi.exclamationmark")
                } description: {
                    Text(message)
                } actions: {
                    Button("Try again") { Task { await libraryStore.refresh(for: profileID) } }
                }
            } else if isEmpty {
                ContentUnavailableView {
                    Label(collection.emptyTitle, systemImage: collection.systemImage)
                } description: {
                    Text(collection.emptyMessage)
                } actions: {
                    Button("Browse titles", action: onBrowseTitles)
                }
            } else {
                collectionContent
            }
        }
        .padding(.horizontal, 20)
        .padding(.top, 12)
        .padding(.bottom, 8)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background { CatalogPageSurface().ignoresSafeArea() }
        .navigationBarTitleDisplayMode(.inline)
        .toolbar(.visible, for: .navigationBar)
        .task(id: profileID) { await libraryStore.loadLibrary(for: profileID) }
        .onDisappear {
            resumeTask?.cancel()
            resumeTask = nil
            resolvingItem = nil
        }
        .sheet(item: $selectedTitle) { title in
            TitleDetailSheet(title: title, profileID: profileID, libraryStore: libraryStore) {
                request in
                playbackRequest = request
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

    // MARK: - Collection Content

    @ViewBuilder
    private var collectionContent: some View {
        switch collection {
        case .continueWatching:
            LibraryProgressPages(
                items: libraryStore.continueWatching(),
                resolvingItem: resolvingItem,
                onPlay: resume,
                onInfo: { selectedTitle = $0 }
            )
        case .myList:
            LibrarySavedPages(titles: libraryStore.library.list, onSelect: { selectedTitle = $0 })
        case .history:
            LibraryHistoryPages(
                items: libraryStore.watchHistory,
                resolvingItem: resolvingItem,
                onPlay: resume,
                onInfo: { selectedTitle = $0 }
            )
        }
    }

    // MARK: - Playback

    private func resume(_ item: WatchProgress) {
        guard resolvingItem == nil else { return }
        let timing = PlaybackTiming(
            action: .resume, titleID: item.title.tmdbID, type: item.title.type,
            season: item.season, episode: item.episode
        )
        resolvingItem = item.libraryID
        resumeTask = Task { @MainActor in
            defer {
                resolvingItem = nil
                resumeTask = nil
            }
            do {
                let request = try await ResumePlaybackHelper.resolveResumeRequest(
                    for: item, profileID: profileID, timing: timing
                )
                try Task.checkCancellation()
                playbackRequest = request
            } catch {
                guard !Task.isCancelled else { return }
                selectedTitle = item.title
            }
        }
    }
}
