import Foundation
import MelloKit

struct UserWatchRecord: Decodable {
    let type: String
    let id: Int
    let season: Int?
    let episode: Int?
    let positionSeconds: Double
    let durationSeconds: Double
    let completed: Bool
    let updatedAt: String
    var identity: String { "\(type)-\(id)-\(season ?? 0)-\(episode ?? 0)" }
    var titleKey: String { "\(type)-\(id)" }
    var date: Date? {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.date(from: updatedAt) ?? ISO8601DateFormatter().date(from: updatedAt)
    }
    var fraction: Double {
        guard durationSeconds.isFinite, positionSeconds.isFinite, durationSeconds > 0 else { return 0 }
        return min(1, max(0, positionSeconds / durationSeconds))
    }
}

struct UserDetails {
    let profileID: String
    let savedCount: Int?
    let history: [UserWatchRecord]?
    let titles: [String: String]
}

private struct UsersReply: Decodable { let profiles: [Profile] }
private struct UserHistoryReply: Decodable { let records: [UserWatchRecord] }
private struct UserTitle: Decodable {
    let id: Int
    let type: String
    let title: String
    var key: String { "\(type)-\(id)" }
}
private struct UserLibraryReply: Decodable { let titles: [UserTitle] }
private struct UserTitleReply: Decodable { let data: UserTitle }
private struct UserInput: Encodable { let name: String; let color: String }

private struct UsersAPI {
    let port: Int

    func send(_ path: String, method: String = "GET", body: Data? = nil) async throws -> Data {
        let base = URL(string: "http://127.0.0.1:\(port)/api/v1/")!
        var request = URLRequest(url: base.appendingPathComponent(path), cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 8)
        request.httpMethod = method
        request.httpBody = body
        if body != nil { request.setValue("application/json", forHTTPHeaderField: "Content-Type") }
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let response = response as? HTTPURLResponse else { throw URLError(.badServerResponse) }
        guard (200..<300).contains(response.statusCode) else {
            throw UserRequestError(status: response.statusCode)
        }
        return data
    }

    func get<T: Decodable>(_ path: String) async throws -> T {
        let data = try await send(path)
        return try decode(data)
    }

    func decode<T: Decodable>(_ data: Data) throws -> T {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(T.self, from: data)
    }
}

private struct UserRequestError: LocalizedError {
    let status: Int
    var errorDescription: String? {
        switch status {
        case 400, 422: return "Check the profile name and try again."
        case 404: return "This profile is no longer available. Refresh the users list."
        default: return "The server couldn’t complete this request. Try again."
        }
    }
}

@MainActor
final class UsersStore: ObservableObject {
    @Published private(set) var profiles: [Profile] = []
    @Published private(set) var hasLoaded = false
    @Published private(set) var isLoading = false
    @Published private(set) var listError: String?
    @Published private(set) var details: UserDetails?
    @Published private(set) var isLoadingDetails = false
    @Published private(set) var isSaving = false
    private var online = false
    private var generation = UUID()
    private var revision = 0
    private var titleCache: [String: String] = [:]

    func setOnline(_ value: Bool) {
        guard value != online else { return }
        online = value
        generation = UUID()
        profiles = []
        details = nil
        hasLoaded = false
        isLoading = false
        isLoadingDetails = false
        listError = nil
        titleCache = [:]
    }

    func refresh(port: Int) async {
        guard online, !isLoading, !isSaving else { return }
        let token = generation
        let revision = revision
        isLoading = true
        defer { if token == generation { isLoading = false } }
        do {
            let reply: UsersReply = try await UsersAPI(port: port).get("profiles")
            guard !Task.isCancelled, token == generation, revision == self.revision else { return }
            profiles = reply.profiles
            hasLoaded = true
            listError = nil
        } catch {
            guard !Task.isCancelled, token == generation, revision == self.revision else { return }
            listError = "Couldn’t load profiles. Your server may be unavailable."
        }
    }

    func loadDetails(profileID: String?, port: Int) async {
        guard online, let profileID else {
            details = nil
            isLoadingDetails = false
            return
        }
        let token = generation
        if details?.profileID != profileID { details = nil }
        isLoadingDetails = true
        defer { if token == generation && !Task.isCancelled { isLoadingDetails = false } }
        let api = UsersAPI(port: port)
        async let library: UserLibraryReply? = try? api.get("profiles/\(profileID)/library")
        async let history: UserHistoryReply? = try? api.get("profiles/\(profileID)/history")
        let (saved, watched) = await (library, history)
        guard !Task.isCancelled, token == generation else { return }
        for title in saved?.titles ?? [] where !title.title.isEmpty { titleCache[title.key] = title.title }
        let recent = watched.map { Array($0.records.prefix(5)) }
        for record in recent ?? [] where titleCache[record.titleKey] == nil {
            if let title: UserTitleReply = try? await api.get("titles/\(record.type)/\(record.id)"), !title.data.title.isEmpty {
                guard !Task.isCancelled, token == generation else { return }
                titleCache[record.titleKey] = title.data.title
            }
        }
        guard !Task.isCancelled, token == generation else { return }
        details = UserDetails(profileID: profileID, savedCount: saved?.titles.count, history: recent, titles: titleCache)
    }

    func save(profile: Profile?, name: String, color: String, port: Int) async throws -> Profile {
        guard online, !isSaving else { throw URLError(.notConnectedToInternet) }
        let token = generation
        revision += 1
        isSaving = true
        defer { isSaving = false }
        let api = UsersAPI(port: port)
        let body = try JSONEncoder().encode(UserInput(name: name, color: color))
        // The edit endpoint changes only name/color, preserving preferences and all viewing data.
        let data = try await api.send(profile.map { "profiles/\($0.id)" } ?? "profiles",
                                      method: profile == nil ? "POST" : "PUT", body: body)
        guard online, token == generation else { throw URLError(.cancelled) }
        let saved: Profile = try api.decode(data)
        if let index = profiles.firstIndex(where: { $0.id == saved.id }) { profiles[index] = saved }
        else { profiles.append(saved) }
        revision += 1
        hasLoaded = true
        return saved
    }

    func delete(_ profile: Profile, port: Int) async throws {
        guard online, !isSaving else { throw URLError(.notConnectedToInternet) }
        let token = generation
        revision += 1
        isSaving = true
        defer { isSaving = false }
        _ = try await UsersAPI(port: port).send("profiles/\(profile.id)", method: "DELETE")
        guard online, token == generation else { throw URLError(.cancelled) }
        profiles.removeAll { $0.id == profile.id }
        if details?.profileID == profile.id { details = nil }
        revision += 1
    }
}
