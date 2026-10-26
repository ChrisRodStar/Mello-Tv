import Foundation
import AVFoundation
private final class PrivateAPIDelegate: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(_ session: URLSession, task: URLSessionTask,
                    willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest,
                    completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        // Keep private API requests on the configured household origin.
        completionHandler(nil)
    }
}

private final class ActiveOriginState: @unchecked Sendable {
    static let shared = ActiveOriginState()
    private let lock = NSLock()
    private var resolved: URL?

    func current(default fallback: URL) -> URL {
        lock.lock()
        defer { lock.unlock() }
        return resolved ?? fallback
    }

    func update(_ url: URL) {
        lock.lock()
        resolved = url
        lock.unlock()
    }
}

struct MelloTVAPI: Sendable {
    let baseURL: URL
    var session: URLSession = Self.privateSession
    var codecSupportOverride: [String: Bool]? = nil

    private var isDynamicOrigin: Bool {
        baseURL.host != "fixture.invalid"
    }

    private var effectiveBaseURL: URL {
        guard isDynamicOrigin else { return baseURL }
        return ActiveOriginState.shared.current(default: ServerAddress.baseURL)
    }

    private var candidateBaseURLs: [URL] {
        guard isDynamicOrigin else { return [baseURL] }
        let primary = ActiveOriginState.shared.current(default: ServerAddress.baseURL)
        var list = [primary, ServerAddress.baseURL]
        if let ts = ServerAddress.tailscaleURL {
            list.append(ts)
        }
        list.append(contentsOf: ServerAddress.tailscaleCandidateURLs)
        list.append(contentsOf: ServerAddress.fallbackURLs)
        list.append(baseURL)

        var seen = Set<URL>()
        var unique = list.filter { seen.insert($0).inserted }

        if ServerAddress.isCellularOnly || ServerAddress.isTailscaleActive {
            unique.sort { a, b in
                let aIsTS = ServerAddress.isTailscaleAddress(a)
                let bIsTS = ServerAddress.isTailscaleAddress(b)
                if aIsTS != bIsTS { return aIsTS }
                return false
            }
        }
        return unique
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

    func home(profileID: String? = nil) async throws -> CatalogResponse {
        try await catalog(category: nil, profileID: profileID)
    }

    func browse(_ type: MediaType, profileID: String? = nil) async throws -> CatalogResponse {
        try await catalog(category: type, profileID: profileID)
    }

    func recommendationPage(category: MediaType?, profileID: String, topic: String = "all") async throws -> CatalogResponse {
        let reply: HomePageReply = try await request("profiles/\(profileID)/home", query: [
            URLQueryItem(name: "type", value: category?.rawValue ?? "all"),
            URLQueryItem(name: "topic", value: topic),
        ])
        let sections = reply.sections.map { CatalogSection(id: $0.id, heading: $0.heading, titles: $0.titles.map(\.view)) }
        let featured = reply.featured.map(\.view)
        var seen = Set<String>()
        let titles = (featured + sections.flatMap(\.titles)).filter { seen.insert($0.id).inserted }
        return CatalogResponse(configured: true, titles: titles, sections: sections, featured: featured)
    }

    private func catalog(category: MediaType?, profileID: String?) async throws -> CatalogResponse {
        if let profileID { return try await recommendationPage(category: category, profileID: profileID) }
        let browse: BrowseReply = try await request("browse")
        let sections = browse.records.map(\.data).filter {
            !$0.id.hasPrefix("onboarding-") && !$0.id.hasPrefix("favorites-")
        }.compactMap { shelf -> CatalogSection? in
            let titles = shelf.titles.filter { category == nil || $0.type == category }.map(\.view)
            return titles.isEmpty ? nil : CatalogSection(id: shelf.id, heading: shelf.heading, titles: titles)
        }
        return CatalogResponse(configured: true, titles: sections.flatMap(\.titles), sections: sections, featured: [])
    }

    func recommendationCategory(id: String, category: MediaType?, profileID: String) async throws -> CatalogSection? {
        let reply: HomePageReply = try await request("profiles/\(profileID)/home", query: [
            URLQueryItem(name: "type", value: category?.rawValue ?? "all"), URLQueryItem(name: "shelf", value: id),
        ])
        return reply.sections.first(where: { $0.id == id }).map { CatalogSection(id: $0.id, heading: $0.heading, titles: $0.titles.map(\.view)) }
    }

    func recommendationChoices(profileID: String) async throws -> RecommendationChoices {
        let reply: FeedbackReply = try await request("profiles/\(profileID)/recommendation-feedback")
        return RecommendationChoices(hidden: reply.hidden.map(\.view), watched: reply.watched.map(\.view),
            ignored: reply.ignored.map(\.view), favorites: reply.favorites.map(\.view), topics: reply.topics)
    }

    func setRecommendationChoice(profileID: String, title: MediaTitle, choice: RecommendationChoice, enabled: Bool) async throws {
        let _: EmptyReply = try await request("profiles/\(profileID)/\(choice.path)/\(title.type.rawValue)/\(title.tmdbID)", method: enabled ? "PUT" : "DELETE")
    }

    func setRecommendationTopicHidden(profileID: String, topicID: String, hidden: Bool) async throws {
        let _: EmptyReply = try await request("profiles/\(profileID)/recommendation-topics/\(topicID)", method: hidden ? "PUT" : "DELETE")
    }

    // Build title suggestions entirely from existing cached API envelopes.
    // Exact seed shelves outrank genre matches; unrelated fallback stays labeled.
    func relatedTitles(to title: MediaTitle, genres: [String], profileID: String) async throws -> CatalogSection {
        guard !profileID.isEmpty else {
            return CatalogSection(id: "related-" + title.id, heading: "More Like This", titles: [])
        }
        async let page: HomePageReply = request("profiles/\(profileID)/home", query: [URLQueryItem(name: "type", value: title.type.rawValue)])
        async let personalized = recommendationShelves(profileID: profileID, type: title.type)
        let (general, personal) = try await (page, personalized)
        try Task.checkCancellation()
        let shelves = personal + general.sections
        let wanted = Set(genres.map(normalizedGenre))
        var candidates: [Int: (title: MediaTitle, score: Int, order: Int)] = [:]
        var order = 0
        for shelf in shelves {
            let exactSeed = shelf.id == "like-\(title.type.rawValue):\(title.tmdbID)"
            let genreShelf = wanted.contains(normalizedGenre(shelf.heading))
            for candidate in shelf.titles where candidate.type == title.type && candidate.id != title.tmdbID {
                defer { order += 1 }
                let view = candidate.view
                guard view.posterURL != nil else { continue }
                let overlap = Set((candidate.genres ?? []).map(normalizedGenre)).intersection(wanted).count
                let score = (exactSeed ? 1000 : 0) + overlap * 20 + (genreShelf ? 10 : 0)
                if let existing = candidates[candidate.id], existing.score >= score { continue }
                candidates[candidate.id] = (view, score, order)
            }
        }
        let ranked = candidates.values.sorted {
            $0.score == $1.score ? $0.order < $1.order : $0.score > $1.score
        }
        let matches = ranked.filter { $0.score > 0 }
        return CatalogSection(id: "\(title.type.rawValue)-suggestions-\(title.tmdbID)",
                              heading: matches.isEmpty ? (personal.isEmpty ? (title.type == .movie ? "Explore Movies" : "Explore TV Shows") : "Recommended for You") : "More Like This",
                              titles: Array((matches.isEmpty ? ranked : matches).prefix(12).map(\.title)))
    }

    private func recommendationShelves(profileID: String, type: MediaType) async throws -> [WireShelf] {
        guard !profileID.isEmpty else { return [] }
        do {
            let reply: SavedRecord<WireRecommendations> = try await request("profiles/\(profileID)/recommendations",
                query: [URLQueryItem(name: "type", value: type.rawValue)])
            return reply.data.sections
        } catch {
            if Task.isCancelled { throw CancellationError() }
            return [] // General cached shelves still work during recommendation outages.
        }
    }

    private func normalizedGenre(_ value: String) -> String {
        let normalized = value.lowercased().filter { $0.isLetter || $0.isNumber }
        return normalized == "scifi" ? "sciencefiction" : normalized
    }

    func search(_ query: String, page: Int = 1) async throws -> SearchResponse {
        let offset = max(0, page - 1) * 50
        let reply: SearchReply = try await request("search", query: [
            URLQueryItem(name: "q", value: query), URLQueryItem(name: "offset", value: String(offset))
        ])
        var titles = reply.records.map { $0.data.view }
        if page == 1 {
            do {
                let discovery: SavedRecord<[WireTitle]> = try await request("search-results", query: [URLQueryItem(name: "q", value: query)])
                titles += discovery.data.map(\.view)
            } catch APIError.serverStatus(404) { }
        }
        var seen = Set<String>()
        return SearchResponse(titles: titles.filter { seen.insert($0.id).inserted }, page: page,
                              totalPages: reply.records.count == reply.limit ? page + 1 : page)
    }

    func details(for title: MediaTitle) async throws -> TitleDetails {
        for attempt in 0..<20 {
            do {
                let reply: SavedRecord<WireTitle> = try await request("titles/\(title.type.rawValue)/\(title.tmdbID)")
                guard reply.data.id == title.tmdbID, reply.data.type == title.type else { throw APIError.invalidData }
                if reply.data.details {
                    return TitleDetails(runtime: reply.data.runtime, genres: reply.data.genres ?? [],
                        seasons: (reply.data.seasons ?? []).map {
                            SeasonSummary(number: $0.season, name: $0.name, episodeCount: $0.episodeCount)
                        })
                }
            } catch APIError.serverStatus(404) {
                // GET queues a bounded metadata refresh; wait for the saved record.
            }
            if attempt < 19 { try await Task.sleep(for: .seconds(1)) }
        }
        throw APIError.serverMessage("Details are still being added. Try again shortly.")
    }

    func seasonEpisodes(tvID: Int, season: Int) async throws -> [Episode] {
        for attempt in 0..<20 {
            do {
                let reply: SavedRecord<WireSeason> = try await request("tv/\(tvID)/seasons/\(season)")
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

    func profiles() async throws -> [Profile] {
        let reply: ProfilesReply = try await request("profiles")
        var profiles = reply.profiles
        for index in profiles.indices {
            let favorites: TitlesReply = try await request("profiles/\(profiles[index].id)/favorites")
            profiles[index].preferences.likedTitles = favorites.titles.map {
                LikedTitle(id: $0.id, type: $0.type, title: $0.title)
            }
        }
        return profiles
    }

    func createProfile(_ profile: Profile) async throws -> [Profile] {
        let _: Profile = try await request("profiles", method: "POST", body: ["name": profile.name, "color": profile.color])
        return try await profiles()
    }

    func updateProfile(_ profile: Profile) async throws -> [Profile] {
        let path = "profiles/\(profile.id)"
        let _: Profile = try await request(path, method: "PUT", body: ["name": profile.name, "color": profile.color])
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

    func deleteProfile(id: String) async throws -> [Profile] {
        let _: EmptyReply = try await request("profiles/\(id)", method: "DELETE")
        return try await profiles()
    }

    func touchActivity(profileID: String) async {
        let _: EmptyReply? = try? await request("profiles/\(profileID)/activity", method: "POST")
    }

    func genreArtwork() async throws -> [String: GenreArtwork] {
        let reply: BrowseReply = try await request("browse")
        var result: [String: GenreArtwork] = [:]
        for shelf in reply.records.map(\.data) where shelf.id.hasPrefix("onboarding-") {
            if let title = shelf.titles.first {
                result[String(shelf.id.dropFirst("onboarding-".count))] = GenreArtwork(
                    imageUrl: artworkURL(title.backdrop ?? title.poster), title: title.title)
            }
        }
        return result
    }

    func onboardingTitles(preferences: ProfilePreferences) async throws -> [MediaTitle] {
        let reply: BrowseReply = try await request("browse")
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

    func onboardingGenerate(preferences: ProfilePreferences) async throws -> CatalogResponse {
        // The catalog owns asynchronous recommendation generation after preferences save.
        try await home()
    }

    // Exact resume reads avoid loading/hydrating the household's entire history
    // just to draw progress on the visible episode cards.
    func episodeProgress(profileID: String, titleID: Int, season: Int, episode: Int) async throws -> Double? {
        do {
            let saved: WireProgress = try await request("profiles/\(profileID)/resume", query: [
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

    func library(for profileID: String) async throws -> ProfileLibrary {
        async let saved: TitlesReply = request("profiles/\(profileID)/library")
        var history: [WireProgress] = []
        var offset = 0
        while offset < 20000 {
            let page: HistoryReply = try await request("profiles/\(profileID)/history", query: [URLQueryItem(name: "offset", value: String(offset))])
            history += page.records
            if page.records.count < page.limit { break }
            offset += page.records.count
        }
        let titles = try await saved.titles.map(\.view)
        var known = Dictionary(titles.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        var progress: [WatchProgress] = []
        for item in history {
            let key = "\(item.type.rawValue)-\(item.id)"
            let title: MediaTitle
            if let existing = known[key] { title = existing }
            else {
                let reply: SavedRecord<WireTitle> = try await request("titles/\(item.type.rawValue)/\(item.id)")
                title = reply.data.view
                known[key] = title
            }
            progress.append(WatchProgress(title: title, season: item.season ?? 0, episode: item.episode ?? 0,
                episodeTitle: "", position: item.positionSeconds, duration: item.durationSeconds,
                watchedSeconds: item.watchedSeconds, completed: item.completed,
                updatedAt: parsedDate(item.updatedAt)?.timeIntervalSince1970 ?? 0))
        }
        return ProfileLibrary(list: titles, progress: progress)
    }

    func addToMyList(profileID: String, title: MediaTitle) async throws -> ProfileLibrary {
        let _: EmptyReply = try await request("profiles/\(profileID)/library/\(title.type.rawValue)/\(title.tmdbID)", method: "PUT")
        return try await library(for: profileID)
    }

    func removeFromMyList(profileID: String, type: MediaType, titleID: Int) async throws -> ProfileLibrary {
        let _: EmptyReply = try await request("profiles/\(profileID)/library/\(type.rawValue)/\(titleID)", method: "DELETE")
        return try await library(for: profileID)
    }

    func updateProgress(profileID: String, update: WatchProgressUpdate) async throws -> Bool {
        let _: WireProgress = try await request("profiles/\(profileID)/progress", method: "PUT", body: [
            "playback_id": update.sessionId, "sequence": update.sequence,
            "position_seconds": update.position, "duration_seconds": update.duration,
            "watched_seconds": update.watchedSeconds,
        ])
        return true
    }

    func resolvePlayback(request playback: PlaybackRequest, key: String) async throws -> AuthorizedPlayback {
        let codecs = Self.playbackCodecs(overriding: codecSupportOverride)
        var body: [String: Any] = ["type": playback.type.rawValue, "id": playback.titleID,
            "device": ["formats": ["hls", "mp4"], "codecs": codecs,
                       "supports_encrypted_hls": true, "allow_unknown_codecs": false, "allow_unknown_resolution": false,
                       "max_height": 1080]]
        if playback.type == .tv {
            body["season"] = playback.season
            body["episode"] = playback.episode
        }
        if let value = playback.catalogTitle.imdbID { body["imdb_id"] = value }
        if let value = playback.catalogTitle.malID { body["mal_id"] = value }
        playback.timing?.record(.authorizationStarted)
        do {
            let reply: WirePlayback = try await playbackRequest("profiles/\(playback.profileID)/playback", method: "POST", body: body, key: key, timingID: playback.timing?.id)
            let source = try authorized(reply, for: playback)
            playback.timing?.record(.authorizationFinished)
            return source
        } catch {
            playback.timing?.record(.authorizationFailed, flag: Task.isCancelled)
            throw error
        }
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

    func changePlayback(_ source: AuthorizedPlayback, request playback: PlaybackRequest,
                        kind: String, key: String) async throws -> AuthorizedPlayback {
        guard ["renew", "backup", "refresh", "upgrade"].contains(kind) else { throw APIError.invalidData }
        let reply: WirePlayback = try await playbackRequest("profiles/\(playback.profileID)/playback/\(source.id)/\(kind)",
            method: "POST", body: ["expected_revision": source.revision], key: key)
        return try authorized(reply, for: playback)
    }

    private func playbackRequest<Response: Decodable>(_ path: String, method: String,
                                                      body: [String: Any], key: String, timingID: UUID? = nil) async throws -> Response {
        for attempt in 0..<3 {
            do {
                return try await request(path, method: method, body: body, key: key, timingID: timingID)
            } catch {
                try Task.checkCancellation()
                guard attempt < 2, (error as? APIError)?.isRetryablePlayback == true else { throw error }
                // Same key and payload reconcile a lost reply rather than
                // creating another session or advancing a mutation twice.
                try await Task.sleep(for: .milliseconds(300 * (attempt + 1)))
            }
        }
        throw APIError.invalidResponse
    }

    func revokePlayback(_ source: AuthorizedPlayback, request playback: PlaybackRequest) async throws {
        let _: EmptyReply = try await request("profiles/\(playback.profileID)/playback/\(source.id)", method: "DELETE")
    }

    private func authorized(_ reply: WirePlayback, for playback: PlaybackRequest) throws -> AuthorizedPlayback {
        let origin = effectiveBaseURL
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
                                  availableHeights: heights, reloadRequired: reply.reloadRequired ?? false)
    }

    private func executeSingleRequest<Response: Decodable>(to candidate: URL, path: String, method: String,
                                                           bodyData: Data?, query: [URLQueryItem],
                                                           key: String?, timingID: UUID?,
                                                           timeout: TimeInterval? = nil) async throws -> Response {
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
        if isDynamicOrigin {
            ActiveOriginState.shared.update(candidate)
            ServerAddress.markEndpointActive(candidate)
        }

        guard let http = response as? HTTPURLResponse else { throw APIError.invalidResponse }
        guard (200..<300).contains(http.statusCode) else {
            if http.statusCode == 404 { throw APIError.serverStatus(404) }
            let code = (try? JSONDecoder().decode(FailureReply.self, from: data))?.error ?? "unavailable"
            throw APIError.service(status: http.statusCode, code: code, message: Self.message(for: code))
        }
        if http.statusCode == 204, let empty = EmptyReply() as? Response { return empty }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        guard let reply = try? decoder.decode(Response.self, from: data) else { throw APIError.invalidData }
        return reply
    }

    private func request<Response: Decodable>(_ path: String, method: String = "GET", body: [String: Any]? = nil,
                                               query: [URLQueryItem] = [], key: String? = nil, timingID: UUID? = nil,
                                               allowDiscoveryRetry: Bool = true) async throws -> Response {
        guard !path.contains(".."), !path.hasPrefix("/") else { throw APIError.invalidAddress }
        let bodyData: Data? = if let body { try JSONSerialization.data(withJSONObject: body) } else { nil }

        // Fast path: try the current effective origin first
        let primary = effectiveBaseURL
        do {
            return try await executeSingleRequest(to: primary, path: path, method: method,
                                                  bodyData: bodyData, query: query, key: key, timingID: timingID)
        } catch {
            if Task.isCancelled { throw CancellationError() }
            if case APIError.service = error { throw error }
            if case APIError.serverStatus = error { throw error }
            if case APIError.serverMessage = error { throw error }

            // Origin failed with connection/network issue.
            // Immediately trigger parallel resolution via ensureResolved() instead of sequentially hanging for 4s on dead candidates.
            if isDynamicOrigin && allowDiscoveryRetry {
                if let resolved = await ServerAddress.ensureResolved() {
                    ActiveOriginState.shared.update(resolved.primaryURL)
                    return try await request(path, method: method, body: body, query: query,
                                             key: key, timingID: timingID, allowDiscoveryRetry: false)
                }
            }

            // Fallback: try remaining candidates if any
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

            throw APIError.connection("Can’t reach Mello TV. Make sure the Mello TV server is running on your network or Tailscale.")
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
private struct SavedRecord<Value: Decodable>: Decodable { let data: Value }
private struct HomePageReply: Decodable { let sections: [WireShelf]; let featured: [WireTitle] }
private struct FeedbackReply: Decodable { let hidden: [WireTitle]; let watched: [WireTitle]; let ignored: [WireTitle]; let favorites: [WireTitle]; let topics: [String] }
private struct BrowseReply: Decodable { let records: [SavedRecord<WireShelf>] }
private struct ProfilesReply: Decodable { let profiles: [Profile] }
private struct TitlesReply: Decodable { let titles: [WireTitle] }
private struct SearchReply: Decodable { let records: [SavedRecord<WireTitle>]; let offset: Int; let limit: Int }
private struct HistoryReply: Decodable { let records: [WireProgress]; let offset: Int; let limit: Int }
private struct WireShelf: Decodable { let id: String; let heading: String; let titles: [WireTitle] }
private struct WireRecommendations: Decodable { let sections: [WireShelf] }
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
    struct Target: Decodable { let type: MediaType; let id: Int; let season: Int?; let episode: Int? }
}
struct AuthorizedPlayback: Sendable {
    let id: String
    let url: URL
    let format: String
    let expiresAt: Date
    let revision: Int
    let resumeSeconds: Double
    let availableHeights: [Int]
    let reloadRequired: Bool

    init(id: String, url: URL, format: String, expiresAt: Date, revision: Int, resumeSeconds: Double,
         availableHeights: [Int], reloadRequired: Bool = false) {
        self.id = id
        self.url = url
        self.format = format
        self.expiresAt = expiresAt
        self.revision = revision
        self.resumeSeconds = resumeSeconds
        self.availableHeights = availableHeights
        self.reloadRequired = reloadRequired
    }
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

enum APIError: LocalizedError {
    case invalidAddress, invalidResponse, invalidData
    case serverStatus(Int), serverMessage(String), connection(String)
    case service(status: Int, code: String, message: String)

    var isRetryablePlayback: Bool {
        switch self {
        case .connection: true
        case .service(let status, let code, _):
            code == "playback_busy" || status == 502 || status == 504
        default: false
        }
    }

    var isExpiredPlayback: Bool {
        if case .service(_, let code, _) = self { return code == "session_expired" || code == "source_expired" }
        return false
    }

    var requiresSourceRefresh: Bool {
        if case .service(_, let code, _) = self { return code == "source_refresh_required" }
        return false
    }

    var isTerminalMutation: Bool {
        if case .service(_, let code, _) = self {
            return ["invalid_input", "invalid_selection", "unsupported_format", "renew_too_early"].contains(code)
        }
        return false
    }

    var isDefinitiveQualityUpgradeFailure: Bool {
        guard case .service(let status, let code, _) = self else { return false }
        return (status == 422 && ["no_source", "quality_unavailable"].contains(code))
            || (status == 409 && code == "active_quality_unknown")
    }
    var errorDescription: String? {
        switch self {
        case .invalidAddress: "The server address is invalid."
        case .invalidResponse: "The server returned an unexpected response."
        case .invalidData: "The server response couldn’t be read."
        case .serverStatus(404): "This item isn’t cached yet. Try again shortly."
        case .serverStatus(let code): "The server returned an error (\(code))."
        case .serverMessage(let message), .connection(let message), .service(_, _, let message): message
        }
    }
}
