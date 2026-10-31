import Foundation
import SwiftUI
import MelloKit

@MainActor
@Observable
final class LibraryStore {
    private let api: MelloTVAPI

    var library: ProfileLibrary = .empty
    var isLoading = false
    var isSaving = false
    var errorMessage: String?
    private var activeProfileID: String?
    private var loadTask: Task<ProfileLibrary, Error>?
    private var loadGeneration = UUID()

    // Manual status is local to this installation. It is deliberately separate
    // from measured viewing telemetry and never sent as fabricated watch time.
    private static let episodeWatchKey = "family-stream.episode-watch-overrides.v1"
    private var manualWatchedEpisodes: Set<String> = {
        guard let data = UserDefaults.standard.data(forKey: episodeWatchKey),
              let values = try? JSONDecoder().decode(Set<String>.self, from: data) else { return [] }
        return values
    }()

    func hasManualWatchedMark(profileID: String, title: MediaTitle, season: Int, episode: Int) -> Bool {
        let key = episodeWatchStorageKey(profileID: profileID, title: title, season: season, episode: episode)
        if manualWatchedEpisodes.contains(key) { return true }
        let endpoints = [ServerAddress.baseURL] + ServerAddress.fallbackURLs
        for endpoint in endpoints {
            let legacy = episodeWatchStorageKey(profileID: profileID, title: title, season: season, episode: episode, origin: endpoint)
            if manualWatchedEpisodes.contains(legacy) {
                manualWatchedEpisodes.remove(legacy)
                manualWatchedEpisodes.insert(key)
                persistWatchedMarks()
                return true
            }
        }
        return false
    }

    func isEpisodeWatched(profileID: String, title: MediaTitle, season: Int, episode: Int) -> Bool {
        if hasManualWatchedMark(profileID: profileID, title: title, season: season, episode: episode) { return true }
        guard let progress = library.progress.first(where: {
            $0.title.tmdbID == title.tmdbID && $0.title.type == title.type && $0.season == season && $0.episode == episode
        }), progress.position.isFinite, progress.duration.isFinite, progress.duration > 0 else { return false }
        // Exactly 95% is still Watching. The API's completed flag uses a
        // different rule, so detail-screen status follows the actual position.
        return progress.position / progress.duration > 0.95
    }

    func setEpisodeWatched(_ watched: Bool, profileID: String, title: MediaTitle, season: Int, episode: Int) {
        let key = episodeWatchStorageKey(profileID: profileID, title: title, season: season, episode: episode)
        if watched { manualWatchedEpisodes.insert(key) }
        else {
            manualWatchedEpisodes.remove(key)
            for endpoint in [ServerAddress.baseURL] + ServerAddress.fallbackURLs {
                manualWatchedEpisodes.remove(episodeWatchStorageKey(profileID: profileID, title: title, season: season, episode: episode, origin: endpoint))
            }
        }
        persistWatchedMarks()
    }

    private func persistWatchedMarks() {
        if let data = try? JSONEncoder().encode(manualWatchedEpisodes) {
            UserDefaults.standard.set(data, forKey: Self.episodeWatchKey)
        }
    }

    private func episodeWatchStorageKey(profileID: String, title: MediaTitle, season: Int, episode: Int, origin: URL? = nil) -> String {
        // Encode the identity components independently to avoid delimiter collisions.
        let identity = [(origin ?? ServerAddress.serverIdentityURL).absoluteString, profileID, title.type.rawValue,
                        String(title.tmdbID), String(season), String(episode)]
        return (try? JSONEncoder().encode(identity).base64EncodedString()) ?? identity.joined(separator: "|")
    }

    init(api: MelloTVAPI? = nil) {
        self.api = api ?? MelloTVAPI(baseURL: ServerAddress.baseURL)
    }

    func loadLibrary(for profileID: String, forceReload: Bool = false) async {
        if activeProfileID == profileID {
            if let loadTask { _ = try? await loadTask.value; return }
            if !forceReload, !library.list.isEmpty || !library.progress.isEmpty { return }
        } else {
            loadTask?.cancel()
            library = .empty
        }
        activeProfileID = profileID
        let generation = UUID()
        loadGeneration = generation
        isLoading = true
        errorMessage = nil
        let api = api
        let task = Task { try await api.library(for: profileID) }
        loadTask = task
        defer {
            if loadGeneration == generation { isLoading = false; loadTask = nil }
        }
        do {
            let data = try await task.value
            guard !task.isCancelled, loadGeneration == generation, activeProfileID == profileID else { return }
            library = data
        } catch {
            guard !task.isCancelled, loadGeneration == generation else { return }
            errorMessage = "Couldn’t load your library. Try again."
        }
    }

    func refresh(for profileID: String) async {
        await loadLibrary(for: profileID, forceReload: true)
    }

    func toggleMyList(profileID: String, title: MediaTitle) async {
        guard !isSaving else { return }
        isSaving = true

        let inList = isInMyList(title)

        do {
            let updated: ProfileLibrary
            if inList {
                updated = try await api.removeFromMyList(profileID: profileID, type: title.type, titleID: title.tmdbID)
            } else {
                updated = try await api.addToMyList(profileID: profileID, title: title)
            }
            self.library = updated
            self.isSaving = false
        } catch {
            self.isSaving = false
            self.errorMessage = "Couldn’t update My List. Try again."
        }
    }

    func isInMyList(_ title: MediaTitle) -> Bool {
        library.list.contains { $0.tmdbID == title.tmdbID && $0.type == title.type }
    }

    func savedProgress(for title: MediaTitle) -> WatchProgress? {
        library.progress.first { $0.title.tmdbID == title.tmdbID && $0.title.type == title.type }
    }

    func continueWatching(filter: MediaType? = nil) -> [WatchProgress] {
        var seen = Set<String>()
        return library.progress.filter { item in
            let key = "\(item.title.type.rawValue):\(item.title.tmdbID)"
            if seen.contains(key) { return false }
            seen.insert(key)

            if let filter, item.title.type != filter {
                return false
            }

            return item.position >= 10 && (!item.completed || item.title.type == .tv)
        }
    }

    var watchHistory: [WatchProgress] {
        library.progress
    }
}
