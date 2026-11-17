import Foundation
import AVFoundation
import MelloKit

private final class PrivateAPIDelegate: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(_ session: URLSession, task: URLSessionTask,
                    willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest,
                    completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        // Keep private API requests on the configured household origin.
        completionHandler(nil)
    }
}

public struct PlaybackRequest: Hashable, Identifiable, Sendable {
    public let titleID: Int
    public let type: MediaType
    public let title: String
    public let profileID: String
    public let season: Int?
    public let episode: Int?
    public let episodeTitle: String?
    public let catalogTitle: MediaTitle
    public let startFromBeginning: Bool

    public var id: String {
        "\(type.rawValue)-\(titleID)-\(season ?? 0)-\(episode ?? 0)"
    }

    public init(
        titleID: Int,
        type: MediaType,
        title: String,
        profileID: String = "default",
        season: Int? = nil,
        episode: Int? = nil,
        episodeTitle: String? = nil,
        catalogTitle: MediaTitle,
        startFromBeginning: Bool = false
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
    }
}

public struct AuthorizedPlayback: Sendable {
    public let id: String
    public let url: URL
    public let format: String
    public let expiresAt: Date
    public let revision: Int
    public let resumeSeconds: Double
    public let availableHeights: [Int]
    public let reloadRequired: Bool
    public let qualityLimit: Int

    public init(id: String, url: URL, format: String, expiresAt: Date, revision: Int, resumeSeconds: Double,
                availableHeights: [Int], reloadRequired: Bool = false, qualityLimit: Int = 0) {
        self.id = id
        self.url = url
        self.format = format
        self.expiresAt = expiresAt
        self.revision = revision
        self.resumeSeconds = resumeSeconds
        self.availableHeights = availableHeights
        self.reloadRequired = reloadRequired
        self.qualityLimit = qualityLimit
    }
}

/// Networking client communicating with the self-hosted Mello TV server from tvOS.
public struct MelloTVAPI: Sendable {
    public static let shared = MelloTVAPI()

    public let baseURL: URL
    public var session: URLSession = Self.privateSession
    public var codecSupportOverride: [String: Bool]? = nil

    public init(baseURL: URL? = nil, session: URLSession? = nil, codecSupportOverride: [String: Bool]? = nil) {
        self.baseURL = baseURL ?? ServerAddress.baseURL
        self.session = session ?? Self.privateSession
        self.codecSupportOverride = codecSupportOverride
    }

    private var isDynamicOrigin: Bool {
        baseURL.host != "fixture.invalid"
    }

    private var effectiveBaseURL: URL {
        guard isDynamicOrigin else { return baseURL }
        return ServerAddress.baseURL
    }

    private var candidateBaseURLs: [URL] {
        guard isDynamicOrigin else { return [baseURL] }
        let primary = ServerAddress.baseURL
        var list = [primary]
        if let current = ServerAddress.currentBaseURL {
            list.append(current)
        }
        list.append(contentsOf: ServerAddress.fallbackURLs)
        list.append(baseURL)

        var seen = Set<URL>()
        return list.filter { seen.insert($0).inserted }
    }

    private static let privateSession: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpShouldSetCookies = false
        configuration.httpCookieAcceptPolicy = .never
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = 45
        configuration.timeoutIntervalForResource = 60
        configuration.httpMaximumConnectionsPerHost = 4
        return URLSession(configuration: configuration, delegate: PrivateAPIDelegate(), delegateQueue: nil)
    }()

    // MARK: - Server Status
    public func fetchStatus() async throws -> ServerSnapshot {
        try await request("status")
    }

    // MARK: - Active Profile Resolution
    public func resolveProfileID(_ provided: String? = nil) async -> String? {
        if let provided, !provided.isEmpty, provided != "default" {
            return provided
        }
        if let active = await ProfileStore.shared.activeProfileID, !active.isEmpty, active != "default" {
            return active
        }
        await ProfileStore.shared.loadProfiles()
        if let active = await ProfileStore.shared.activeProfileID, !active.isEmpty, active != "default" {
            return active
        }
        if let first = (try? await profiles())?.first?.id, !first.isEmpty, first != "default" {
            return first
        }
        return nil
    }

    // MARK: - Catalog & Media
    public func home(profileID: String? = nil) async throws -> CatalogResponse {
        let pid = await resolveProfileID(profileID)
        return try await catalog(category: nil, profileID: pid)
    }

    public func browse(_ type: MediaType, profileID: String? = nil) async throws -> CatalogResponse {
        let pid = await resolveProfileID(profileID)
        return try await catalog(category: type, profileID: pid)
    }

    public func recommendationPage(category: MediaType?, profileID: String? = nil, topic: String = "all") async throws -> CatalogResponse {
        guard let resolvedID = await resolveProfileID(profileID), !resolvedID.isEmpty else {
            return try await catalog(category: category, profileID: nil)
        }
        let reply: HomePageWireReply = try await request("profiles/\(resolvedID)/home", query: [
            URLQueryItem(name: "type", value: category?.rawValue ?? "all"),
            URLQueryItem(name: "topic", value: topic),
        ])
        let sections = reply.sections.map { CatalogSection(id: $0.id, heading: $0.heading, titles: $0.titles.map(\.view), reason: $0.reason) }
        let featured = reply.featured.map(\.view)
        var seen = Set<String>()
        let titles = (featured + sections.flatMap(\.titles)).filter { seen.insert($0.id).inserted }
        return CatalogResponse(configured: true, titles: titles, sections: sections, featured: featured)
    }

    private func catalog(category: MediaType?, profileID: String?) async throws -> CatalogResponse {
        if let profileID, !profileID.isEmpty, profileID != "default" {
            return try await recommendationPage(category: category, profileID: profileID)
        }
        let browse = try await browseReply()
        let sections = browse.records.map(\.data).filter {
            !$0.id.hasPrefix("onboarding-") && !$0.id.hasPrefix("favorites-")
        }.compactMap { shelf -> CatalogSection? in
            let titles = shelf.titles.filter { category == nil || $0.type == category }.map(\.view)
            return titles.isEmpty ? nil : CatalogSection(id: shelf.id, heading: shelf.heading, titles: titles)
        }
        return CatalogResponse(configured: true, titles: sections.flatMap(\.titles), sections: sections, featured: [])
    }

    private func browseReply() async throws -> BrowseWireReply {
        let delays: [Duration] = [.zero, .seconds(2), .seconds(4), .seconds(7), .seconds(10), .seconds(12), .seconds(12)]
        for delay in delays {
            if delay > .zero { try await Task.sleep(for: delay) }
            try Task.checkCancellation()
            let reply: BrowseWireReply = try await request("browse")
            switch reply.status {
            case nil, .ready: return reply
            case .pending: continue
            case .unavailable:
                throw APIError.serverMessage(reply.reason ?? "Catalog is unavailable. Configure TMDB in Mello TV Mac Settings, then try again.")
            case .failed:
                throw APIError.serverMessage(reply.reason ?? "Catalog titles could not be loaded. Try again.")
            }
        }
        throw APIError.serverMessage("Catalog titles are still being prepared. Try again shortly.")
    }

    public func fetchCatalog() async throws -> [MediaTitle] {
        let response = try await home()
        return response.titles
    }

    public func recommendationCategory(id: String, category: MediaType?, profileID: String? = nil) async throws -> CatalogSection? {
        guard let resolvedID = await resolveProfileID(profileID), !resolvedID.isEmpty else { return nil }
        let reply: HomePageWireReply = try await request("profiles/\(resolvedID)/home", query: [
            URLQueryItem(name: "type", value: category?.rawValue ?? "all"), URLQueryItem(name: "shelf", value: id),
        ])
        return reply.sections.first(where: { $0.id == id }).map { CatalogSection(id: $0.id, heading: $0.heading, titles: $0.titles.map(\.view), reason: $0.reason) }
    }

    public func recommendationChoices(profileID: String? = nil) async throws -> RecommendationChoices {
        guard let resolvedID = await resolveProfileID(profileID), !resolvedID.isEmpty else {
            return RecommendationChoices(hidden: [], watched: [], ignored: [], favorites: [], topics: [])
        }
        let reply: FeedbackReply = try await request("profiles/\(resolvedID)/recommendation-feedback")
        return RecommendationChoices(hidden: reply.hidden.map(\.view), watched: reply.watched.map(\.view),
            ignored: reply.ignored.map(\.view), favorites: reply.favorites.map(\.view), topics: reply.topics)
    }

    public func setRecommendationChoice(profileID: String? = nil, title: MediaTitle, choice: RecommendationChoice, enabled: Bool) async throws {
        guard let resolvedID = await resolveProfileID(profileID), !resolvedID.isEmpty else { return }
        let _: EmptyReply = try await request("profiles/\(resolvedID)/\(choice.path)/\(title.type.rawValue)/\(title.tmdbID)", method: enabled ? "PUT" : "DELETE")
    }

    public func setRecommendationTopicHidden(profileID: String? = nil, topicID: String, hidden: Bool) async throws {
        guard let resolvedID = await resolveProfileID(profileID), !resolvedID.isEmpty else { return }
        let _: EmptyReply = try await request("profiles/\(resolvedID)/recommendation-topics/\(topicID)", method: hidden ? "PUT" : "DELETE")
    }

    public func relatedTitles(to title: MediaTitle, genres _: [String], profileID _: String? = nil) async throws -> CatalogSection {
        let reply: RelatedTitlesReply = try await request("titles/\(title.type.rawValue)/\(title.tmdbID)/related")
        guard let titles = reply.titles else {
            throw APIError.serverMessage(reply.reason ?? "Couldn’t load related titles.")
        }
        return CatalogSection(id: "related-\(title.id)", heading: "More Like This", titles: titles.map(\.view))
    }

    // MARK: - Search
    public func search(_ query: String, page: Int = 1, pageSize: Int = 20) async throws -> SearchResponse {
        let offset = max(0, page - 1) * max(1, pageSize)
        let delays: [Duration] = [.zero, .seconds(2), .seconds(4), .seconds(7), .seconds(10), .seconds(12), .seconds(12)]
        for delay in delays {
            if delay > .zero { try await Task.sleep(for: delay) }
            try Task.checkCancellation()
            let reply: SearchReply = try await request("search", query: [
                URLQueryItem(name: "q", value: query), URLQueryItem(name: "offset", value: String(offset))
            ])
            if let status = reply.status, try catalogStatusError(status, reason: reply.reason) { continue }
            var titles = reply.records.map { $0.data.view }
            if page == 1 {
                do {
                    let discovery: SavedWireRecord<[WireTitle]>
                    if titles.isEmpty {
                        discovery = try await request("search-results", query: [URLQueryItem(name: "q", value: query)])
                    } else {
                        discovery = try await executeSingleRequest(to: effectiveBaseURL, path: "search-results", method: "GET",
                            bodyData: nil, query: [URLQueryItem(name: "q", value: query)], key: nil, timingID: nil,
                            timeout: 2, pendingAttempt: 7)
                    }
                    if let status = discovery.status, try catalogStatusError(status, reason: discovery.reason) { continue }
                    titles += discovery.data.map(\.view)
                } catch {
                    try Task.checkCancellation()
                }
            }
            var seen = Set<String>()
            return SearchResponse(titles: titles.filter { seen.insert($0.id).inserted }, page: page,
                                  totalPages: reply.records.count == reply.limit && offset < 10000 ? page + 1 : page,
                                  limit: reply.limit)
        }
        throw APIError.serverMessage("Catalog titles are still being prepared. Try again shortly.")
    }

    public func search(query: String) async throws -> [MediaTitle] {
        let response = try await search(query, page: 1, pageSize: 50)
        return response.titles
    }

    private func catalogStatusError(_ status: CatalogGenerationStatus.State, reason: String?) throws -> Bool {
        switch status {
        case .ready: return false
        case .pending: return true
        case .unavailable:
            throw APIError.serverMessage(reason ?? "Catalog is unavailable. Configure TMDB in Mello TV Mac Settings, then try again.")
        case .failed:
            throw APIError.serverMessage(reason ?? "Catalog titles could not be loaded. Try again.")
        }
    }

    // MARK: - Title & Season Details
    public func timingIMDbID(for title: MediaTitle) async throws -> String? {
        let reply: SavedWireRecord<WireTitle> = try await executeSingleRequest(to: effectiveBaseURL,
            path: "titles/\(title.type.rawValue)/\(title.tmdbID)", method: "GET", bodyData: nil,
            query: [], key: nil, timingID: nil, timeout: 2, pendingAttempt: 7)
        guard reply.data.id == title.tmdbID, reply.data.type == title.type else { throw APIError.invalidData }
        return reply.data.imdbId
    }

    public func details(for title: MediaTitle) async throws -> TitleDetails {
        for attempt in 0..<20 {
            do {
                let reply: SavedWireRecord<WireTitle> = try await request("titles/\(title.type.rawValue)/\(title.tmdbID)")
                guard reply.data.id == title.tmdbID, reply.data.type == title.type else { throw APIError.invalidData }
                if reply.data.details {
                    return TitleDetails(runtime: reply.data.runtime, genres: reply.data.genres ?? [],
                        seasons: (reply.data.seasons ?? []).map {
                            SeasonSummary(number: $0.season, name: $0.name, episodeCount: $0.episodeCount)
                        })
                }
            } catch APIError.serverStatus(404) {
            }
            if attempt < 19 { try await Task.sleep(for: .seconds(1)) }
        }
        throw APIError.serverMessage("Details are still being added. Try again shortly.")
    }

    public func fetchTitleDetails(type: MediaType, id: Int) async throws -> MediaTitle {
        let reply: SavedWireRecord<WireTitle> = try await request("titles/\(type.rawValue)/\(id)")
        return reply.data.view
    }

    public func seasonEpisodes(tvID: Int, season: Int) async throws -> [Episode] {
        for attempt in 0..<20 {
            do {
                let reply: SavedWireRecord<WireSeason> = try await request("tv/\(tvID)/seasons/\(season)")
                guard reply.data.showId == tvID, reply.data.season == season else { throw APIError.invalidData }
                return reply.data.episodes.map {
                    Episode(number: $0.episode, name: $0.name, overview: $0.overview,
                            runtime: $0.runtime > 0 ? $0.runtime : nil, stillURL: artworkURL($0.still))
                }
            } catch APIError.serverStatus(404) {
                if attempt < 19 { try await Task.sleep(for: .seconds(1)) }
            }
        }
        throw APIError.serverMessage("Episodes are still being added. Try again shortly.")
    }

    public func fetchSeasonDetails(showId: Int, seasonNumber: Int) async throws -> SeasonDetails {
        let episodes = try await seasonEpisodes(tvID: showId, season: seasonNumber)
        return SeasonDetails(seasonNumber: seasonNumber, episodes: episodes)
    }

    // MARK: - Stream Resolution
    public func resolveStreamURL(type: MediaType, id: Int, season: Int? = nil, episode: Int? = nil, profileID: String = "default") async throws -> URL {
        let resolvedPID = (await resolveProfileID(profileID)) ?? ""
        let key = UUID().uuidString
        let codecs = Self.playbackCodecs(overriding: codecSupportOverride)
        var body: [String: Any] = [
            "type": type.rawValue,
            "id": id,
            "device": [
                "formats": ["hls", "mp4"],
                "codecs": codecs,
                "supports_encrypted_hls": true,
                "allow_unknown_codecs": false,
                "allow_unknown_resolution": false,
                "max_height": 1080
            ]
        ]
        if type == .tv {
            if let season { body["season"] = season }
            if let episode { body["episode"] = episode }
        }
        let reply: WirePlayback = try await playbackRequest("profiles/\(resolvedPID)/playback", method: "POST", body: body, key: key)
        let origin = reply.responseOrigin ?? effectiveBaseURL
        guard reply.url.hasPrefix("/stream/"), !reply.url.hasPrefix("//"),
              let url = URL(string: reply.url, relativeTo: origin)?.absoluteURL,
              url.scheme == origin.scheme, url.host == origin.host, url.port == origin.port else {
            throw APIError.invalidData
        }
        return url
    }

    public func resolvePlayback(request playback: PlaybackRequest, key: String) async throws -> AuthorizedPlayback {
        let resolvedPID = (await resolveProfileID(playback.profileID)) ?? ""
        let settings = (try? await playbackSettings(profileID: resolvedPID)) ?? ProfilePlaybackSettings()
        try Task.checkCancellation()
        let ceiling = settings.maxHeight == 0 ? 1080 : settings.maxHeight
        let codecs = Self.playbackCodecs(overriding: codecSupportOverride)
        var body: [String: Any] = [
            "type": playback.type.rawValue,
            "id": playback.titleID,
            "device": [
                "formats": ["hls", "mp4"],
                "codecs": codecs,
                "supports_encrypted_hls": true,
                "allow_unknown_codecs": false,
                "allow_unknown_resolution": false,
                "max_height": ceiling
            ]
        ]
        if playback.type == .tv {
            if let s = playback.season { body["season"] = s }
            if let e = playback.episode { body["episode"] = e }
        }
        if let value = playback.catalogTitle.imdbID { body["imdb_id"] = value }
        if let value = playback.catalogTitle.malID { body["mal_id"] = value }

        let reply: WirePlayback = try await playbackRequest("profiles/\(resolvedPID)/playback", method: "POST", body: body, key: key)
        return try authorized(reply, for: playback, qualityLimit: settings.maxHeight)
    }

    private static func playbackCodecs(overriding override: [String: Bool]?) -> [String] {
        var codecs = ["avc1", "mp4a"]
        for (family, codecString) in [
            ("hvc1", "hvc1.1.6.L120.B0"),
            ("hev1", "hev1.1.6.L120.B0"),
        ] {
            let supported = override?[family]
                ?? AVURLAsset.isPlayableExtendedMIMEType("video/mp4; codecs=\"\(codecString)\"")
            if supported { codecs.append(family) }
        }
        return codecs
    }

    public func changePlayback(_ source: AuthorizedPlayback, request playback: PlaybackRequest,
                               kind: String, key: String) async throws -> AuthorizedPlayback {
        guard ["renew", "backup", "refresh", "upgrade"].contains(kind) else { throw APIError.invalidData }
        let resolvedPID = (await resolveProfileID(playback.profileID)) ?? ""
        let reply: WirePlayback = try await playbackRequest("profiles/\(resolvedPID)/playback/\(source.id)/\(kind)",
            method: "POST", body: ["expected_revision": source.revision], key: key)
        return try authorized(reply, for: playback, qualityLimit: source.qualityLimit)
    }

    private func playbackRequest<Response: Decodable>(_ path: String, method: String,
                                                      body: [String: Any], key: String, timingID: UUID? = nil) async throws -> Response {
        for attempt in 0..<3 {
            do {
                return try await request(path, method: method, body: body, key: key, timingID: timingID)
            } catch {
                try Task.checkCancellation()
                guard attempt < 2, (error as? APIError)?.isRetryablePlayback == true else { throw error }
                try await Task.sleep(for: .milliseconds(300 * (attempt + 1)))
            }
        }
        throw APIError.invalidResponse
    }

    public func revokePlayback(_ source: AuthorizedPlayback, request playback: PlaybackRequest) async throws {
        let resolvedPID = (await resolveProfileID(playback.profileID)) ?? ""
        try await revokePlayback(id: source.id, profileID: resolvedPID)
    }

    public func revokePlayback(id: String, profileID: String? = nil) async throws {
        let resolvedPID = (await resolveProfileID(profileID)) ?? ""
        let _: EmptyReply = try await request("profiles/\(resolvedPID)/playback/\(id)", method: "DELETE")
    }

    private func authorized(_ reply: WirePlayback, for playback: PlaybackRequest, qualityLimit: Int) throws -> AuthorizedPlayback {
        let origin = reply.responseOrigin ?? effectiveBaseURL
        let heights = reply.availableHeights ?? []
        guard heights.count <= 32, Set(heights).count == heights.count,
              heights.allSatisfy({ $0 > 0 && $0 <= 16384 }), reply.format == "hls" || heights.isEmpty else { throw APIError.invalidData }
        guard ["hls", "mp4"].contains(reply.format), reply.title.type == playback.type, reply.title.id == playback.titleID,
              playback.type != .tv || (reply.title.season == playback.season && reply.title.episode == playback.episode),
              reply.url.hasPrefix("/stream/"), !reply.url.hasPrefix("//"),
              let url = URL(string: reply.url, relativeTo: origin)?.absoluteURL,
              url.scheme == origin.scheme, url.host == origin.host, url.port == origin.port,
              url.user == nil, url.password == nil, url.fragment == nil,
              let expires = parsedDate(reply.expiresAt), expires > Date() else { throw APIError.invalidData }
        return AuthorizedPlayback(id: reply.id, url: url, format: reply.format, expiresAt: expires,
                                  revision: reply.revision, resumeSeconds: reply.resumeSeconds,
                                  availableHeights: heights, reloadRequired: reply.reloadRequired ?? false, qualityLimit: qualityLimit)
    }

    // MARK: - Activity Reporting
    public func putPlaybackActivity(sessionID: String, body: [String: Any]) async throws {
        let _: EmptyReply = try await request("playback-activity/\(sessionID)", method: "PUT", body: body)
    }

    public func deletePlaybackActivity(sessionID: String) async throws {
        let _: EmptyReply = try await request("playback-activity/\(sessionID)", method: "DELETE")
    }

    public func updatePlaybackActivity(
        sessionId: String,
        profileId: String? = nil,
        title: String,
        season: Int? = nil,
        episode: Int? = nil,
        state: String,
        positionSeconds: Double,
        durationSeconds: Double? = nil
    ) async {
        let resolvedPID = (await resolveProfileID(profileId)) ?? (profileId ?? "")
        var body: [String: Any] = [
            "session_id": sessionId,
            "profile_id": resolvedPID,
            "title": title,
            "device": "Apple TV",
            "state": state,
            "position_seconds": positionSeconds
        ]
        if let season { body["season"] = season }
        if let episode { body["episode"] = episode }
        if let durationSeconds { body["duration_seconds"] = durationSeconds }

        try? await putPlaybackActivity(sessionID: sessionId, body: body)
    }

    public func stopPlaybackActivity(sessionId: String) async {
        try? await deletePlaybackActivity(sessionID: sessionId)
    }

    public func touchActivity(profileID: String? = nil) async {
        guard let resolvedPID = await resolveProfileID(profileID), !resolvedPID.isEmpty else { return }
        let _: EmptyReply? = try? await request("profiles/\(resolvedPID)/activity", method: "POST")
    }

    // MARK: - Profiles
    public func profiles() async throws -> [Profile] {
        let reply: ProfilesReply = try await request("profiles")
        return reply.profiles
    }

    public func fetchProfiles() async throws -> [Profile] {
        try await profiles()
    }

    public func profileForPreferenceEditing(id: String) async throws -> Profile {
        guard var profile = try await profiles().first(where: { $0.id == id }) else {
            throw APIError.serverMessage("This profile is no longer available.")
        }
        let favorites: TitlesReply = try await request("profiles/\(id)/favorites")
        profile.preferences.likedTitles = favorites.titles.map {
            LikedTitle(id: $0.id, type: $0.type, title: $0.title)
        }
        return profile
    }

    public func createProfile(_ profile: Profile) async throws -> Profile {
        try await request("profiles", method: "POST", body: ["name": profile.name, "color": profile.color])
    }

    public func updateProfileIdentity(_ profile: Profile) async throws -> [Profile] {
        let _: Profile = try await request("profiles/\(profile.id)", method: "PUT", body: ["name": profile.name, "color": profile.color])
        return try await profiles()
    }

    public func updateProfile(_ profile: Profile) async throws -> [Profile] {
        let path = "profiles/\(profile.id)"
        guard profile.preferences.likedTitles.count <= 50 else {
            throw APIError.serverMessage("Choose up to 50 favorites and try again.")
        }
        let existing: TitlesReply = try await request(path + "/favorites")
        let wanted = Set(profile.preferences.likedTitles.map { "\($0.type.rawValue):\($0.id)" })
        for favorite in existing.titles where !wanted.contains("\(favorite.type.rawValue):\(favorite.id)") {
            let _: EmptyReply = try await request(path + "/favorites/\(favorite.type.rawValue)/\(favorite.id)", method: "DELETE")
        }
        let before = Set(existing.titles.map { "\($0.type.rawValue):\($0.id)" })
        for favorite in profile.preferences.likedTitles where !before.contains("\(favorite.type.rawValue):\(favorite.id)") {
            let _: EmptyReply = try await request(path + "/favorites/\(favorite.type.rawValue)/\(favorite.id)", method: "PUT")
        }
        let _: Profile = try await request(path + "/preferences", method: "PUT", body: [
            "completed": profile.preferences.completed, "format": profile.preferences.format,
            "genres": profile.preferences.genres,
        ])
        return try await profiles()
    }

    public func deleteProfile(id: String) async throws -> [Profile] {
        let _: EmptyReply = try await request("profiles/\(id)", method: "DELETE")
        return try await profiles()
    }

    public func playbackSettings(profileID: String? = nil) async throws -> ProfilePlaybackSettings {
        guard let resolvedPID = await resolveProfileID(profileID), !resolvedPID.isEmpty else {
            return ProfilePlaybackSettings()
        }
        return try await request("profiles/\(resolvedPID)/playback-settings")
    }

    public func savePlaybackSettings(_ settings: ProfilePlaybackSettings, profileID: String? = nil) async throws {
        guard let resolvedPID = await resolveProfileID(profileID), !resolvedPID.isEmpty else { return }
        let _: ProfilePlaybackSettings = try await request("profiles/\(resolvedPID)/playback-settings", method: "PUT", body: [
            "audio_language": settings.audioLanguage, "subtitle_language": settings.subtitleLanguage,
            "subtitles_enabled": settings.subtitlesEnabled, "autoplay_next": settings.autoplayNext,
            "max_height": settings.maxHeight
        ])
    }

    public func catalogStatus(profileID: String? = nil) async throws -> CatalogGenerationStatus {
        guard let resolvedPID = await resolveProfileID(profileID), !resolvedPID.isEmpty else {
            return CatalogGenerationStatus(status: .ready)
        }
        return try await request("profiles/\(resolvedPID)/catalog-status")
    }

    public func waitForCatalogReady(profileID: String? = nil) async throws {
        guard let resolvedPID = await resolveProfileID(profileID), !resolvedPID.isEmpty else { return }
        let delays: [Duration] = [.zero, .seconds(2), .seconds(4), .seconds(7), .seconds(10), .seconds(12), .seconds(12)]
        for delay in delays {
            if delay > .zero { try await Task.sleep(for: delay) }
            try Task.checkCancellation()
            let state = try await catalogStatus(profileID: resolvedPID)
            switch state.status {
            case .ready: return
            case .pending: continue
            case .unavailable:
                throw APIError.serverMessage(state.reason ?? "Catalog is unavailable. Configure TMDB in Mello TV Mac Settings, then try again.")
            case .failed:
                throw APIError.serverMessage(state.reason ?? "Recommendations could not be generated. Try again.")
            }
        }
        throw APIError.serverMessage("Recommendations are still being prepared. Try again shortly.")
    }

    public func onboardingTitles(preferences: ProfilePreferences, profileID: String? = nil) async throws -> [MediaTitle] {
        guard let resolvedPID = await resolveProfileID(profileID), !resolvedPID.isEmpty else { return [] }
        try await waitForCatalogReady(profileID: resolvedPID)
        let reply = try await browseReply()
        let shelves = reply.records.map(\.data)
        var buckets: [[WireTitle]] = []
        for (index, genre) in preferences.genres.enumerated() {
            let formats = preferences.format == "both" ? (index.isMultiple(of: 2) ? ["movie", "tv"] : ["tv", "movie"]) : [preferences.format]
            let lists = formats.map { format in
                (shelves.first { $0.id == "favorites-\(format)-\(genre)" }
                 ?? (format == "movie" ? shelves.first { $0.id == "onboarding-\(genre)" } : nil))?.titles.filter {
                    $0.type.rawValue == format && artworkURL($0.poster) != nil
                } ?? []
            }
            var bucket: [WireTitle] = []
            for position in 0..<(lists.map(\.count).max() ?? 0) {
                for list in lists where position < list.count { bucket.append(list[position]) }
            }
            buckets.append(bucket)
        }
        var seen = Set<String>()
        var choices: [MediaTitle] = []
        for index in 0..<(buckets.map(\.count).max() ?? 0) {
            for bucket in buckets where index < bucket.count && choices.count < 48 {
                let title = bucket[index].view
                if seen.insert(title.id).inserted { choices.append(title) }
            }
        }
        return choices
    }

    public func genreArtwork() async throws -> [String: GenreArtwork] {
        let reply = try await browseReply()
        var result: [String: GenreArtwork] = [:]
        let genreIDs = Set(GenreOption.all.map(\.id))
        for shelf in reply.records.map(\.data) {
            let genre: String?
            if shelf.id.hasPrefix("onboarding-") {
                genre = String(shelf.id.dropFirst("onboarding-".count))
            } else if shelf.id.hasPrefix("favorites-movie-") {
                genre = String(shelf.id.dropFirst("favorites-movie-".count))
            } else if shelf.id.hasPrefix("favorites-tv-") {
                genre = String(shelf.id.dropFirst("favorites-tv-".count))
            } else {
                genre = nil
            }
            guard let genre, genreIDs.contains(genre) else { continue }
            if let title = shelf.titles.first {
                if result[genre] == nil || result[genre]?.imageUrl == nil {
                    result[genre] = GenreArtwork(imageUrl: artworkURL(title.backdrop ?? title.poster), title: title.title)
                }
            }
        }
        return result
    }

    // MARK: - Library & Continue Watching
    public func fetchContinueWatching(profileID: String? = nil) async throws -> [WatchProgress] {
        guard let resolvedPID = await resolveProfileID(profileID), !resolvedPID.isEmpty else { return [] }
        let lib = try await library(for: resolvedPID)
        var seen = Set<String>()
        return lib.progress.filter { item in
            let key = "\(item.title.type.rawValue):\(item.title.tmdbID)"
            if seen.contains(key) { return false }
            seen.insert(key)
            return item.position >= 10 && (!item.completed || item.title.type == .tv)
        }
    }

    public func episodeProgress(profileID: String? = nil, titleID: Int, season: Int, episode: Int) async throws -> Double? {
        guard let resolvedPID = await resolveProfileID(profileID), !resolvedPID.isEmpty else { return nil }
        do {
            let saved: WireProgress = try await request("profiles/\(resolvedPID)/resume", query: [
                URLQueryItem(name: "type", value: "tv"),
                URLQueryItem(name: "id", value: String(titleID)),
                URLQueryItem(name: "season", value: String(season)),
                URLQueryItem(name: "episode", value: String(episode)),
            ])
            guard saved.type == .tv, saved.id == titleID, saved.season == season, saved.episode == episode,
                  saved.positionSeconds.isFinite, saved.durationSeconds.isFinite,
                  saved.positionSeconds >= 0, saved.durationSeconds > 0 else { throw APIError.invalidData }
            return min(1, max(0, saved.positionSeconds / saved.durationSeconds))
        } catch APIError.serverStatus(404) {
            return nil
        }
    }

    public func library(for profileID: String? = nil) async throws -> ProfileLibrary {
        guard let resolvedPID = await resolveProfileID(profileID), !resolvedPID.isEmpty else { return .empty }
        async let saved: TitlesReply = request("profiles/\(resolvedPID)/library")
        var history: [WireProgress] = []
        var offset = 0
        while offset < 20000 {
            let page: HistoryReply = try await request("profiles/\(resolvedPID)/history", query: [URLQueryItem(name: "offset", value: String(offset))])
            history += page.records
            if page.records.isEmpty || page.records.count < page.limit { break }
            offset += page.records.count
        }
        let titles = try await saved.titles.map(\.view)
        var known = Dictionary(titles.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        var missing: [WireProgress] = []
        var seen = Set(known.keys)
        for item in history {
            if seen.insert("\(item.type.rawValue)-\(item.id)").inserted { missing.append(item) }
        }
        for start in stride(from: 0, to: missing.count, by: 4) {
            try Task.checkCancellation()
            let batch = Array(missing[start..<min(start + 4, missing.count)])
            let hydrated = await withTaskGroup(of: (String, MediaTitle?).self) { group in
                for item in batch {
                    group.addTask {
                        let key = "\(item.type.rawValue)-\(item.id)"
                        let reply: SavedWireRecord<WireTitle>? = try? await request("titles/\(item.type.rawValue)/\(item.id)")
                        return (key, reply?.data.view)
                    }
                }
                var result: [String: MediaTitle] = [:]
                for await (key, title) in group { if let title { result[key] = title } }
                return result
            }
            known.merge(hydrated) { _, new in new }
        }
        try Task.checkCancellation()
        let progress = history.map { item in
            let key = "\(item.type.rawValue)-\(item.id)"
            let title = known[key] ?? MediaTitle(tmdbID: item.id, type: item.type,
                title: "\(item.type.displayName) \(item.id)")
            return WatchProgress(title: title, season: item.season ?? 0, episode: item.episode ?? 0,
                episodeTitle: "", position: item.positionSeconds, duration: item.durationSeconds,
                watchedSeconds: item.watchedSeconds, completed: item.completed,
                updatedAt: parsedDate(item.updatedAt)?.timeIntervalSince1970 ?? 0)
        }
        return ProfileLibrary(list: titles, progress: progress)
    }

    public func fetchLibrary(profileId: String? = nil) async throws -> ProfileLibrary {
        try await library(for: profileId)
    }

    public func addToMyList(profileID: String? = nil, title: MediaTitle) async throws -> ProfileLibrary {
        guard let resolvedPID = await resolveProfileID(profileID), !resolvedPID.isEmpty else { return .empty }
        let _: EmptyReply = try await request("profiles/\(resolvedPID)/library/\(title.type.rawValue)/\(title.tmdbID)", method: "PUT")
        return try await library(for: resolvedPID)
    }

    public func removeFromMyList(profileID: String? = nil, type: MediaType, titleID: Int) async throws -> ProfileLibrary {
        guard let resolvedPID = await resolveProfileID(profileID), !resolvedPID.isEmpty else { return .empty }
        let _: EmptyReply = try await request("profiles/\(resolvedPID)/library/\(type.rawValue)/\(titleID)", method: "DELETE")
        return try await library(for: resolvedPID)
    }

    public func addToWatchlist(profileId: String? = nil, type: MediaType, id: Int) async throws {
        guard let resolvedPID = await resolveProfileID(profileId), !resolvedPID.isEmpty else { return }
        let _: EmptyReply = try await request("profiles/\(resolvedPID)/library/\(type.rawValue)/\(id)", method: "PUT")
    }

    public func removeFromWatchlist(profileId: String? = nil, type: MediaType, id: Int) async throws {
        guard let resolvedPID = await resolveProfileID(profileId), !resolvedPID.isEmpty else { return }
        let _: EmptyReply = try await request("profiles/\(resolvedPID)/library/\(type.rawValue)/\(id)", method: "DELETE")
    }

    public func updateProgress(profileID: String? = nil, update: WatchProgressUpdate) async throws -> Bool {
        guard let resolvedPID = await resolveProfileID(profileID), !resolvedPID.isEmpty else { return false }
        let _: WireProgress = try await request("profiles/\(resolvedPID)/progress", method: "PUT", body: [
            "playback_id": update.sessionId, "sequence": update.sequence,
            "position_seconds": update.position, "duration_seconds": update.duration,
            "watched_seconds": update.watchedSeconds,
        ])
        return true
    }

    // MARK: - Core Request Engine
    private func executeSingleRequest<Response: Decodable>(to candidate: URL, path: String, method: String,
                                                           bodyData: Data?, query: [URLQueryItem],
                                                           key: String?, timingID: UUID?,
                                                           timeout: TimeInterval? = nil,
                                                           pendingAttempt: Int = 0) async throws -> Response {
        guard ["http", "https"].contains(candidate.scheme) else { throw APIError.invalidAddress }
        var components = URLComponents(url: candidate.appending(path: "api/v1/" + path), resolvingAgainstBaseURL: false)
        components?.queryItems = query.isEmpty ? nil : query
        guard let url = components?.url, url.host == candidate.host else { throw APIError.invalidAddress }

        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue(candidate.absoluteString.trimmingCharacters(in: CharacterSet(charactersIn: "/")), forHTTPHeaderField: "Origin")

        if let timeout {
            request.timeoutInterval = timeout
        } else if method == "POST" && path.hasSuffix("/playback") {
            request.timeoutInterval = 35
        } else {
            request.timeoutInterval = 4.0
        }

        if let key { request.setValue(key, forHTTPHeaderField: "Idempotency-Key") }
        if let timingID { request.setValue(timingID.uuidString.lowercased(), forHTTPHeaderField: "X-Playback-Trace-ID") }
        if let bodyData {
            request.httpBody = bodyData
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }

        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse else { throw APIError.invalidResponse }
        let statusPayload = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        let isMetadataPending = http.statusCode == 202 ||
            (statusPayload?["status"] as? String == "pending" &&
             statusPayload?["key"] != nil && statusPayload?["records"] == nil && statusPayload?["data"] == nil)
        if isMetadataPending {
            guard method == "GET", pendingAttempt < 7 else {
                throw APIError.serverMessage("Catalog data is still being prepared. Try again shortly.")
            }
            let delays: [Duration] = [.seconds(1), .seconds(2), .seconds(3), .seconds(5), .seconds(8), .seconds(12), .seconds(12)]
            try await Task.sleep(for: delays[pendingAttempt])
            try Task.checkCancellation()
            return try await executeSingleRequest(to: candidate, path: path, method: method,
                bodyData: bodyData, query: query, key: key, timingID: timingID, timeout: timeout,
                pendingAttempt: pendingAttempt + 1)
        }
        guard (200..<300).contains(http.statusCode) else {
            if http.statusCode == 404 { throw APIError.serverStatus(404) }
            let code = (try? JSONDecoder().decode(FailureReply.self, from: data))?.error ?? "unavailable"
            throw APIError.service(status: http.statusCode, code: code, message: Self.message(for: code))
        }
        if http.statusCode == 204, let empty = EmptyReply() as? Response {
            if isDynamicOrigin { ServerAddress.markEndpointActive(candidate) }
            return empty
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        if let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let status = value["status"] as? String,
           status == "unavailable" || status == "failed" {
            throw APIError.serverMessage(value["reason"] as? String ?? "Catalog data could not be loaded. Try again.")
        }
        guard let reply = try? decoder.decode(Response.self, from: data) else { throw APIError.invalidData }
        if isDynamicOrigin { ServerAddress.markEndpointActive(candidate) }
        if var playback = reply as? WirePlayback {
            playback.responseOrigin = candidate
            return (playback as? Response) ?? reply
        }
        return reply
    }

    private func request<Response: Decodable>(_ path: String, method: String = "GET", body: [String: Any]? = nil,
                                               query: [URLQueryItem] = [], key: String? = nil, timingID: UUID? = nil,
                                               allowDiscoveryRetry: Bool = true) async throws -> Response {
        guard !path.contains(".."), !path.hasPrefix("/") else { throw APIError.invalidAddress }
        let bodyData: Data? = if let body { try JSONSerialization.data(withJSONObject: body) } else { nil }

        let primary = effectiveBaseURL
        do {
            return try await executeSingleRequest(to: primary, path: path, method: method,
                                                  bodyData: bodyData, query: query, key: key, timingID: timingID)
        } catch {
            if Task.isCancelled { throw CancellationError() }
            if case APIError.service = error { throw error }
            if case APIError.serverStatus = error { throw error }
            if case APIError.serverMessage = error { throw error }

            if method == "POST", key == nil {
                throw APIError.connection("Couldn’t confirm the request. Refresh before trying again.")
            }
            if isDynamicOrigin && allowDiscoveryRetry {
                if await ServerAddress.ensureResolved() != nil {
                    return try await request(path, method: method, body: body, query: query,
                                             key: key, timingID: timingID, allowDiscoveryRetry: false)
                }
            }

            let remaining = candidateBaseURLs.filter { $0 != primary }
            for candidate in remaining {
                do {
                    return try await executeSingleRequest(to: candidate, path: path, method: method,
                                                          bodyData: bodyData, query: query, key: key, timingID: timingID,
                                                          timeout: 4.0)
                } catch {
                    if Task.isCancelled { throw CancellationError() }
                    continue
                }
            }

            throw APIError.connection("Can’t reach Mello TV. Make sure the Mello TV server is running on your network.")
        }
    }

    private static func message(for code: String) -> String {
        switch code {
        case "list_limit": "Choose up to 50 favorites and try again."
        case "invalid_request": "Check the information and try again."
        case "no_source": "No playable source is available for this title."
        case "vpn_unavailable": "Playback is temporarily unavailable. Try again shortly."
        case "playback_busy", "overloaded": "The server is busy. Try again shortly."
        case "state_limit": "The streaming server has reached its playback limit. Try again shortly."
        case "session_expired", "revoked": "This playback session has ended. Start playback again."
        case "source_refresh_required": "The playback source has expired. Start playback again."
        default: "Couldn’t complete that request. Try again."
        }
    }
}

// MARK: - Wire Transfer DTOs
private struct EmptyReply: Decodable {}
private struct FailureReply: Decodable {
    let error: String

    private enum CodingKeys: String, CodingKey { case code, error }

    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        error = try values.decodeIfPresent(String.self, forKey: .code)
            ?? values.decodeIfPresent(String.self, forKey: .error)
            ?? "unavailable"
    }
}
private struct SavedWireRecord<Value: Decodable>: Decodable {
    let data: Value
    let status: CatalogGenerationStatus.State?
    let reason: String?
}
private struct HomePageWireReply: Decodable { let sections: [WireShelf]; let featured: [WireTitle] }
private struct FeedbackReply: Decodable { let hidden: [WireTitle]; let watched: [WireTitle]; let ignored: [WireTitle]; let favorites: [WireTitle]; let topics: [String] }
private struct BrowseWireReply: Decodable {
    let records: [SavedWireRecord<WireShelf>]
    let status: CatalogGenerationStatus.State?
    let reason: String?
}
private struct ProfilesReply: Decodable { let profiles: [Profile] }
private struct TitlesReply: Decodable { let titles: [WireTitle] }
private struct SearchReply: Decodable {
    let records: [SavedWireRecord<WireTitle>]
    let offset: Int
    let limit: Int
    let status: CatalogGenerationStatus.State?
    let reason: String?
}
private struct HistoryReply: Decodable { let records: [WireProgress]; let offset: Int; let limit: Int }
private struct WireShelf: Decodable { let id: String; let heading: String; let titles: [WireTitle]; let reason: String? }
private struct RelatedTitlesReply: Decodable { let titles: [WireTitle]?; let reason: String? }
private struct WireSeasonSummary: Decodable { let season: Int; let name: String; let episodeCount: Int }
private struct WireTitle: Decodable {
    let id: Int
    let type: MediaType
    let title: String
    let overview: String
    let date: String?
    let rating: Double
    let poster: String?
    let backdrop: String?
    let imdbId: String?
    let malId: Int?
    let runtime: Int?
    let genres: [String]?
    let seasons: [WireSeasonSummary]?
    let details: Bool
    var view: MediaTitle {
        MediaTitle(tmdbID: id, type: type, title: title, overview: overview, year: String((date ?? "").prefix(4)),
                   rating: rating, backdropURL: artworkURL(backdrop), posterURL: artworkURL(poster), imdbID: imdbId, malID: malId, genres: genres ?? [])
    }
}
private struct WireSeason: Decodable { let showId: Int; let season: Int; let episodes: [WireEpisode] }
private struct WireEpisode: Decodable { let episode: Int; let name: String; let overview: String; let runtime: Int; let still: String? }
private struct WireProgress: Decodable {
    let type: MediaType
    let id: Int
    let season: Int?
    let episode: Int?
    let positionSeconds: Double
    let durationSeconds: Double
    let watchedSeconds: Double
    let completed: Bool
    let updatedAt: String
}
private struct WirePlayback: Decodable {
    let id: String
    let title: Target
    let url: String
    let format: String
    let expiresAt: String
    let revision: Int
    let resumeSeconds: Double
    let availableHeights: [Int]?
    let reloadRequired: Bool?
    var responseOrigin: URL? = nil
    enum CodingKeys: String, CodingKey {
        case id, title, url, format, expiresAt, revision, resumeSeconds, availableHeights, reloadRequired
    }
    struct Target: Decodable { let type: MediaType; let id: Int; let season: Int?; let episode: Int? }
}

private func artworkURL(_ raw: String?) -> URL? {
    guard let raw, !raw.isEmpty else { return nil }
    let normalized = raw
        .replacingOccurrences(of: #"^/(?:api|internal)/v1/artwork/"#, with: "https://image.tmdb.org/t/p/", options: .regularExpression)
        .replacingOccurrences(of: #"^/api/artwork/"#, with: "https://image.tmdb.org/t/p/", options: .regularExpression)
    guard let url = URL(string: normalized), url.scheme == "https", url.host == "image.tmdb.org",
          url.user == nil, url.password == nil, url.query == nil, url.fragment == nil,
          normalized.range(of: #"^https://image\.tmdb\.org/t/p/(w300|w500|w780|w1280)/[A-Za-z0-9_-]+\.(jpg|jpeg|png|webp)$"#, options: .regularExpression) != nil else { return nil }
    return url
}

private func parsedDate(_ raw: String) -> Date? {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return formatter.date(from: raw) ?? ISO8601DateFormatter().date(from: raw)
}
