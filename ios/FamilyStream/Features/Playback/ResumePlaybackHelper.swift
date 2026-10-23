import Foundation

enum ResumePlaybackHelper {
    static func resolveResumeRequest(
        for item: WatchProgress,
        profileID: String,
        api: FamilyStreamAPI = FamilyStreamAPI(baseURL: ServerAddress.baseURL),
        timing: PlaybackTiming? = nil
    ) async throws -> PlaybackRequest {
        timing?.record(.resumeMetadataStarted)
        var resolved = false
        defer { timing?.record(.resumeMetadataFinished, flag: resolved) }
        if item.title.type == .movie {
            resolved = true
            return PlaybackRequest(
                titleID: item.title.tmdbID,
                type: .movie,
                title: item.title.title,
                profileID: profileID,
                catalogTitle: item.title,
                timing: timing
            )
        }

        var season = item.season
        var episode = item.episode
        var episodeTitle = item.episodeTitle

        if item.completed {
            let currentSeasonEpisodes = try await api.seasonEpisodes(tvID: item.title.tmdbID, season: season)
            if let nextInSeason = currentSeasonEpisodes.first(where: { $0.number > episode }) {
                episode = nextInSeason.number
                episodeTitle = nextInSeason.name
            } else {
                let details = try await api.details(for: item.title)
                let nextSeason = details.seasons
                    .filter { $0.number > season && $0.episodeCount > 0 }
                    .sorted { $0.number < $1.number }
                    .first

                guard let nextSeason else {
                    throw APIError.serverMessage("You’ve finished the available episodes.")
                }

                season = nextSeason.number
                let nextSeasonEpisodes = try await api.seasonEpisodes(tvID: item.title.tmdbID, season: season)
                guard let firstOfNext = nextSeasonEpisodes.first else {
                    throw APIError.serverMessage("No next episode is available.")
                }

                episode = firstOfNext.number
                episodeTitle = firstOfNext.name
            }
        }

        resolved = true
        return PlaybackRequest(
            titleID: item.title.tmdbID,
            type: .tv,
            title: item.title.title,
            profileID: profileID,
            season: season,
            episode: episode,
            episodeTitle: episodeTitle,
            catalogTitle: item.title,
            timing: timing
        )
    }
}
