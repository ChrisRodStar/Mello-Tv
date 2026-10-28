import Foundation
import SwiftUI

@MainActor
@Observable
final class LibraryStore {
    private let api: MelloTVAPI

    var library: ProfileLibrary = .empty
    var isLoading = false
    var isSaving = false
    var errorMessage: String?
    private var activeProfileID: String?

    // Manual status is local to this installation. It is deliberately separate
    // from measured viewing telemetry and never sent as fabricated watch time.
    private static let episodeWatchKey = "family-stream.episode-watch-overrides.v1"
    private var manualWatchedEpisodes: Set<String> = {
        guard let data = UserDefaults.standard.data(forKey: episodeWatchKey),
              let values = try? JSONDecoder().decode(Set<String>.self, from: data) else { return [] }
        return values
    }()

    func hasManualWatchedMark(profileID: String, title: MediaTitle, season: Int, episode: Int) -> Bool {
        manualWatchedEpisodes.contains(episodeWatchStorageKey(profileID: profileID, title: title, season: season, episode: episode))
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
        else { manualWatchedEpisodes.remove(key) }
        if let data = try? JSONEncoder().encode(manualWatchedEpisodes) {
            UserDefaults.standard.set(data, forKey: Self.episodeWatchKey)
        }
    }

    private func episodeWatchStorageKey(profileID: String, title: MediaTitle, season: Int, episode: Int) -> String {
        // Encode the identity components independently to avoid delimiter collisions.
        let identity = [ServerAddress.baseURL.absoluteString, profileID, title.type.rawValue,
                        String(title.tmdbID), String(season), String(episode)]
        return (try? JSONEncoder().encode(identity).base64EncodedString()) ?? identity.joined(separator: "|")
    }

    init(api: MelloTVAPI? = nil) {
        self.api = api ?? MelloTVAPI(baseURL: ServerAddress.baseURL)
    }

    func loadLibrary(for profileID: String, forceReload: Bool = false) async {
        if !forceReload, activeProfileID == profileID, !library.list.isEmpty {
            return
        }

        activeProfileID = profileID
        isLoading = true
        errorMessage = nil

        do {
            let data = try await api.library(for: profileID)
            self.library = data
            self.isLoading = false
        } catch {
            self.isLoading = false
            self.errorMessage = "Couldn’t load your library. Try again."
        }
    }

    func refresh(for profileID: String) async {
        do {
            let data = try await api.library(for: profileID)
            self.library = data
            self.errorMessage = nil
        } catch {
            // Keep existing data on refresh error
        }
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
