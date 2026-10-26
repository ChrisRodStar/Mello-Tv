import Foundation

struct PlaybackRequest: Hashable, Identifiable, Sendable {
    let titleID: Int
    let type: MediaType
    let title: String
    let profileID: String
    let season: Int?
    let episode: Int?
    let episodeTitle: String?
    let catalogTitle: MediaTitle
    let startFromBeginning: Bool
    var timing: PlaybackTiming?

    init(
        titleID: Int,
        type: MediaType,
        title: String,
        profileID: String,
        season: Int? = nil,
        episode: Int? = nil,
        episodeTitle: String? = nil,
        catalogTitle: MediaTitle,
        startFromBeginning: Bool = false,
        timing: PlaybackTiming? = nil
    ) {
        self.titleID = titleID
        self.type = type
        self.title = title
        self.profileID = profileID
        self.season = season
        self.episode = episode
        self.episodeTitle = episodeTitle
        self.catalogTitle = catalogTitle
        self.startFromBeginning = startFromBeginning
        self.timing = timing
    }

    // Timing is deliberately excluded from equality, hashing and cache keys.
    static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.titleID == rhs.titleID && lhs.type == rhs.type && lhs.title == rhs.title &&
        lhs.profileID == rhs.profileID && lhs.season == rhs.season && lhs.episode == rhs.episode &&
        lhs.episodeTitle == rhs.episodeTitle && lhs.catalogTitle == rhs.catalogTitle &&
        lhs.startFromBeginning == rhs.startFromBeginning
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(titleID); hasher.combine(type); hasher.combine(title)
        hasher.combine(profileID); hasher.combine(season); hasher.combine(episode)
        hasher.combine(episodeTitle); hasher.combine(catalogTitle); hasher.combine(startFromBeginning)
    }

    func traced(_ action: PlaybackTiming.Action, using trace: PlaybackTiming? = nil) -> Self {
        var copy = self
        copy.timing = trace ?? PlaybackTiming(action: action, titleID: titleID, type: type, season: season, episode: episode)
        return copy
    }

    var id: String { cacheKey }

    // Restart/resume is a local seek choice; both use the same authorization.
    var preparationKey: String { progressKey }

    var cacheKey: String { progressKey + (startFromBeginning ? "-from-start" : "") }

    var progressKey: String {
        if type == .tv {
            return "\(profileID)-\(type.rawValue)-\(titleID)-s\(season ?? 0)e\(episode ?? 0)"
        }
        return "\(profileID)-\(type.rawValue)-\(titleID)"
    }
}

// Shared by explicit Next Episode and near-end preparation. Metadata lookup does
// not resolve providers, create a playback session or fetch video bytes.
enum NextEpisodeResolver {
    static func next(after request: PlaybackRequest, api: MelloTVAPI) async throws -> PlaybackRequest? {
        guard request.type == .tv, let season = request.season, let episode = request.episode else { return nil }
        let episodes = try await api.seasonEpisodes(tvID: request.titleID, season: season)
        try Task.checkCancellation()
        if let next = episodes.sorted(by: { $0.number < $1.number }).first(where: { $0.number > episode }) {
            return makeRequest(next, season: season, after: request)
        }
        let details = try await api.details(for: request.catalogTitle)
        try Task.checkCancellation()
        guard let nextSeason = details.seasons.filter({ $0.number > season && $0.episodeCount > 0 })
            .sorted(by: { $0.number < $1.number }).first else { return nil }
        let nextEpisodes = try await api.seasonEpisodes(tvID: request.titleID, season: nextSeason.number)
        try Task.checkCancellation()
        guard let first = nextEpisodes.sorted(by: { $0.number < $1.number }).first else {
            throw APIError.serverMessage("The next season’s episodes aren’t available yet.")
        }
        return makeRequest(first, season: nextSeason.number, after: request)
    }

    private static func makeRequest(_ episode: Episode, season: Int, after request: PlaybackRequest) -> PlaybackRequest {
        PlaybackRequest(titleID: request.titleID, type: .tv, title: request.title,
            profileID: request.profileID, season: season, episode: episode.number,
            episodeTitle: episode.name, catalogTitle: request.catalogTitle)
    }
}
