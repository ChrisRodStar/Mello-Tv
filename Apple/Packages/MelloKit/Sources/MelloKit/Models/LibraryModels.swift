import Foundation

public struct WatchProgress: Codable, Hashable, Sendable {
    public let title: MediaTitle
    public let season: Int
    public let episode: Int
    public let episodeTitle: String
    public let position: Double
    public let duration: Double
    public let watchedSeconds: Double
    public var completed: Bool
    public var manuallyCompleted: Bool?
    public var continueWatchingRemovedAt: Int?
    public let updatedAt: Double

    public init(
        title: MediaTitle,
        season: Int,
        episode: Int,
        episodeTitle: String,
        position: Double,
        duration: Double,
        watchedSeconds: Double,
        completed: Bool,
        updatedAt: Double,
        manuallyCompleted: Bool? = nil,
        continueWatchingRemovedAt: Int? = nil
    ) {
        self.title = title
        self.season = season
        self.episode = episode
        self.episodeTitle = episodeTitle
        self.position = position
        self.duration = duration
        self.watchedSeconds = watchedSeconds
        self.completed = completed
        self.manuallyCompleted = manuallyCompleted
        self.continueWatchingRemovedAt = continueWatchingRemovedAt
        self.updatedAt = updatedAt
    }
}

public struct WatchProgressUpdate: Codable, Sendable {
    public let title: MediaTitle
    public let sessionId: String
    public let sequence: Int
    public let season: Int
    public let episode: Int
    public let episodeTitle: String
    public let position: Double
    public let duration: Double
    public let watchedSeconds: Double

    public init(
        title: MediaTitle,
        sessionId: String,
        sequence: Int,
        season: Int,
        episode: Int,
        episodeTitle: String,
        position: Double,
        duration: Double,
        watchedSeconds: Double
    ) {
        self.title = title
        self.sessionId = sessionId
        self.sequence = sequence
        self.season = season
        self.episode = episode
        self.episodeTitle = episodeTitle
        self.position = position
        self.duration = duration
        self.watchedSeconds = watchedSeconds
    }
}

public struct ProfileLibrary: Codable, Sendable {
    public var list: [MediaTitle]
    public var progress: [WatchProgress]

    public static let empty = ProfileLibrary(list: [], progress: [])

    public init(list: [MediaTitle] = [], progress: [WatchProgress] = []) {
        self.list = list
        self.progress = progress
    }
}

public struct SimpleStatusResponse: Codable, Sendable {
    public let ok: Bool?

    public init(ok: Bool? = nil) {
        self.ok = ok
    }
}

