import Foundation
import MelloKit

/// Networking client communicating with the self-hosted Mello TV server.
public final class MelloTVAPI: Sendable {
    public static let shared = MelloTVAPI()

    private let session: URLSession

    private init() {
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 8.0
        config.timeoutIntervalForResource = 30.0
        self.session = URLSession(configuration: config)
    }

    private func makeURL(_ path: String, queryItems: [URLQueryItem]? = nil) throws -> URL {
        guard let base = ServerAddress.currentBaseURL else {
            throw APIError.invalidAddress
        }
        var comps = URLComponents(url: base.appendingPathComponent(path), resolvingAgainstBaseURL: true)
        if let queryItems {
            comps?.queryItems = queryItems
        }
        guard let finalURL = comps?.url else {
            throw APIError.invalidAddress
        }
        return finalURL
    }

    // MARK: - Server Status
    public func fetchStatus() async throws -> ServerSnapshot {
        let url = try makeURL("api/v1/status")
        let (data, response) = try await session.data(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.serverStatus((response as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode(ServerSnapshot.self, from: data)
    }

    // MARK: - Catalog & Media
    public func fetchCatalog() async throws -> [MediaTitle] {
        let url = try makeURL("api/v1/catalog")
        let (data, response) = try await session.data(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.serverStatus((response as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode([MediaTitle].self, from: data)
    }

    public func fetchTitleDetails(type: MediaType, id: Int) async throws -> MediaTitle {
        let url = try makeURL("api/v1/titles/\(type.rawValue)/\(id)")
        let (data, response) = try await session.data(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.serverStatus((response as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode(MediaTitle.self, from: data)
    }

    public func fetchSeasonDetails(showId: Int, seasonNumber: Int) async throws -> SeasonDetails {
        let url = try makeURL("api/v1/titles/tv/\(showId)/seasons/\(seasonNumber)")
        let (data, response) = try await session.data(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.serverStatus((response as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode(SeasonDetails.self, from: data)
    }

    public func search(query: String) async throws -> [MediaTitle] {
        let url = try makeURL("api/v1/search", queryItems: [URLQueryItem(name: "q", value: query)])
        let (data, response) = try await session.data(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.serverStatus((response as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode([MediaTitle].self, from: data)
    }

    // MARK: - Stream Resolution
    public func resolveStreamURL(type: MediaType, id: Int, season: Int? = nil, episode: Int? = nil) async throws -> URL {
        var query: [URLQueryItem] = []
        if let season { query.append(URLQueryItem(name: "season", value: String(season))) }
        if let episode { query.append(URLQueryItem(name: "episode", value: String(episode))) }

        let url = try makeURL("api/v1/titles/\(type.rawValue)/\(id)/stream", queryItems: query.isEmpty ? nil : query)
        let (data, response) = try await session.data(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.serverStatus((response as? HTTPURLResponse)?.statusCode ?? -1)
        }

        struct StreamResponse: Codable {
            let streamURL: String?
            let relayPath: String?

            enum CodingKeys: String, CodingKey {
                case streamURL = "stream_url"
                case relayPath = "relay_path"
            }
        }

        let streamResp = try JSONDecoder().decode(StreamResponse.self, from: data)
        if let relay = streamResp.relayPath, let base = ServerAddress.currentBaseURL {
            return base.appendingPathComponent(relay)
        } else if let stream = streamResp.streamURL, let parsed = URL(string: stream) {
            return parsed
        }
        throw APIError.invalidData
    }

    // MARK: - Profiles & Library
    public func fetchProfiles() async throws -> [Profile] {
        let url = try makeURL("api/v1/profiles")
        let (data, response) = try await session.data(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.serverStatus((response as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode([Profile].self, from: data)
    }

    public func fetchLibrary(profileId: String) async throws -> ProfileLibrary {
        let url = try makeURL("api/v1/profiles/\(profileId)/library")
        let (data, response) = try await session.data(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.serverStatus((response as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode(ProfileLibrary.self, from: data)
    }

    // MARK: - Watchlist Management
    public func addToWatchlist(profileId: String = "default", type: MediaType, id: Int) async throws {
        let url = try makeURL("api/v1/profiles/\(profileId)/library/\(type.rawValue)/\(id)")
        var request = URLRequest(url: url)
        request.httpMethod = "PUT"
        let (_, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            throw APIError.serverStatus((response as? HTTPURLResponse)?.statusCode ?? -1)
        }
    }

    public func removeFromWatchlist(profileId: String = "default", type: MediaType, id: Int) async throws {
        let url = try makeURL("api/v1/profiles/\(profileId)/library/\(type.rawValue)/\(id)")
        var request = URLRequest(url: url)
        request.httpMethod = "DELETE"
        let (_, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            throw APIError.serverStatus((response as? HTTPURLResponse)?.statusCode ?? -1)
        }
    }

    // MARK: - Playback Activity Reporting
    public func updatePlaybackActivity(
        sessionId: String,
        profileId: String,
        title: String,
        season: Int? = nil,
        episode: Int? = nil,
        state: String,
        positionSeconds: Double,
        durationSeconds: Double? = nil
    ) async {
        guard let url = try? makeURL("api/v1/playback-activity/\(sessionId)") else { return }

        struct ActivityPayload: Encodable {
            let session_id: String
            let profile_id: String
            let title: String
            let device: String
            let season: Int?
            let episode: Int?
            let state: String
            let position_seconds: Double
            let duration_seconds: Double?
        }

        let payload = ActivityPayload(
            session_id: sessionId,
            profile_id: profileId,
            title: title,
            device: "Apple TV",
            season: season,
            episode: episode,
            state: state,
            position_seconds: positionSeconds,
            duration_seconds: durationSeconds
        )

        var request = URLRequest(url: url)
        request.httpMethod = "PUT"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try? JSONEncoder().encode(payload)

        _ = try? await session.data(for: request)
    }

    public func stopPlaybackActivity(sessionId: String) async {
        guard let url = try? makeURL("api/v1/playback-activity/\(sessionId)") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "DELETE"
        _ = try? await session.data(for: request)
    }
}
