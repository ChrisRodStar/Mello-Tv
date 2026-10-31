import Foundation
import AVFoundation
import SwiftUI
import MelloKit

private struct FixtureReply: Sendable { let status: Int; let data: Data }
private actor ClientFixture {
    var paths: [String] = []
    var progressStatus = 503
    var progressDelay = false
    var slowLibrary = false
    var profilesFail = false
    var qualityHeight = 0
    var requestedHeight: Int?
    var progressSequences: [Int] = []
    var deletes = 0
    var ambiguousCreation = false
    var createdProfile = false
    var createCount = 0

    func configure(progressStatus: Int = 503, progressDelay: Bool = false,
                   slowLibrary: Bool = false, profilesFail: Bool = false, qualityHeight: Int = 0,
                   ambiguousCreation: Bool = false) {
        self.progressStatus = progressStatus; self.progressDelay = progressDelay
        self.slowLibrary = slowLibrary; self.profilesFail = profilesFail; self.qualityHeight = qualityHeight
        paths = []; progressSequences = []; deletes = 0; requestedHeight = nil
        self.ambiguousCreation = ambiguousCreation; createdProfile = false; createCount = 0
    }
    func respond(_ request: URLRequest) async throws -> FixtureReply {
        let path = request.url!.path
        paths.append(path)
        var bytes = request.httpBody ?? Data()
        if bytes.isEmpty, let stream = request.httpBodyStream {
            stream.open(); defer { stream.close() }
            var buffer = [UInt8](repeating: 0, count: 4096)
            while stream.hasBytesAvailable {
                let size = stream.read(&buffer, maxLength: buffer.count)
                if size <= 0 { break }
                bytes.append(contentsOf: buffer.prefix(size))
            }
        }
        let body = (try? JSONSerialization.jsonObject(with: bytes)) as? [String: Any] ?? [:]
        let title: [String: Any] = ["id": 1, "type": "movie", "title": "Saved title", "overview": "", "rating": 0, "details": true]
        let profile: [String: Any] = ["id": "p", "name": "Fixture", "color": "#446e88",
            "preferences": ["format": "both", "genres": [], "completed": true]]
        let status: Int
        let reply: [String: Any]
        if path == "/api/v1/profiles", request.httpMethod == "POST", ambiguousCreation {
            createCount += 1; createdProfile = true
            return FixtureReply(status: 0, data: Data()) // committed, then lost reply
        } else if request.httpMethod == "DELETE" {
            deletes += 1
            return FixtureReply(status: 204, data: Data())
        } else if path == "/api/v1/profiles" {
            status = profilesFail ? 503 : 200
            var profiles = [profile]
            if createdProfile {
                profiles.append(["id": "created", "name": "New fixture", "color": "#446e88",
                    "preferences": ["format": "both", "genres": [], "completed": true]])
            }
            reply = profilesFail ? ["error": "unavailable"] : ["profiles": profiles]
        } else if path.hasSuffix("/favorites") || path.hasSuffix("/search-results") {
            status = 503; reply = ["error": "unavailable"]
        } else if path.hasSuffix("/playback-settings") {
            status = 200; reply = ["audio_language": "", "subtitle_language": "", "subtitles_enabled": false,
                "autoplay_next": false, "max_height": qualityHeight]
        } else if path.hasSuffix("/playback") {
            requestedHeight = (body["device"] as? [String: Any])?["max_height"] as? Int
            status = 200; reply = ["id": "session", "title": ["type": "movie", "id": 1],
                "url": "/stream/session/entry?st=fixture", "format": "hls", "available_heights": [360],
                "expires_at": "2099-01-01T00:00:00Z", "revision": 1, "resume_seconds": 0]
        } else if path.hasSuffix("/library") {
            if slowLibrary { try await Task.sleep(for: .milliseconds(200)) }
            status = 200; reply = ["titles": [title]]
        } else if path.hasSuffix("/history") {
            status = 200; reply = ["records": [["type": "movie", "id": 2, "position_seconds": 10,
                "duration_seconds": 100, "watched_seconds": 10, "completed": false,
                "updated_at": "2026-10-31T12:00:00Z"]], "offset": 0, "limit": 100]
        } else if path.hasSuffix("/search") {
            status = 200; reply = ["records": [["data": title]], "offset": 0, "limit": 20]
        } else if path.hasSuffix("/progress") {
            let sequence = body["sequence"] as? Int ?? -1
            progressSequences.append(sequence)
            let responseStatus = progressStatus
            if progressDelay { try await Task.sleep(for: .milliseconds(150)) }
            status = responseStatus
            reply = status == 200 ? ["type": "movie", "id": 1,
                "position_seconds": body["position_seconds"] ?? 0,
                "duration_seconds": body["duration_seconds"] ?? 100,
                "watched_seconds": body["watched_seconds"] ?? 0, "completed": false,
                "updated_at": "2026-10-31T12:00:00Z"] : ["error": status == 409 ? "session_expired" : "unavailable"]
        } else {
            status = 404; reply = ["error": "not_found"]
        }
        return FixtureReply(status: status, data: try JSONSerialization.data(withJSONObject: reply))
    }
}

private final class ClientProtocol: URLProtocol, @unchecked Sendable {
    static let fixture = ClientFixture()
    private let lock = NSRecursiveLock()
    private var stopped = false
    private var loadingTask: Task<Void, Never>?
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        loadingTask = Task {
            do {
                let reply = try await Self.fixture.respond(request)
                deliver(reply)
            } catch { }
        }
    }
    private func deliver(_ reply: FixtureReply) {
        lock.lock(); defer { lock.unlock() }
        guard !stopped else { return }
        if reply.status == 0 {
            client?.urlProtocol(self, didFailWithError: URLError(.networkConnectionLost))
            return
        }
        client?.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: reply.status,
            httpVersion: nil, headerFields: ["Content-Type": "application/json"])!, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: reply.data)
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {
        lock.lock(); stopped = true; lock.unlock()
        loadingTask?.cancel()
    }
}

@main struct ClientReliabilityChecks {
    @MainActor static func main() async throws {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [ClientProtocol.self]
        let api = MelloTVAPI(baseURL: URL(string: "https://fixture.invalid")!, session: URLSession(configuration: configuration))
        let fixture = ClientProtocol.fixture
        let title = MediaTitle(tmdbID: 1, type: .movie, title: "Fixture")
        let request = PlaybackRequest(titleID: 1, type: .movie, title: title.title, profileID: "p", catalogTitle: title)
        let profiles = try await api.profiles()
        precondition(profiles.count == 1)
        do { _ = try await api.profileForPreferenceEditing(id: "p"); preconditionFailure("unavailable favorites must not become an empty editable list") }
        catch { }
        print("PASS profile entry survives favorites outages while preference editing requires authoritative favorites")

        let library = try await api.library(for: "p")
        precondition(library.list.count == 1 && library.progress.count == 1 && library.progress[0].title.tmdbID == 2)
        let search = try await api.search("Fixture")
        precondition(search.titles.count == 1)
        print("PASS missing historical metadata retains saved titles/history and failed discovery retains cached search")

        // Isolate all preferences to this command-line executable and restore them.
        let defaults = UserDefaults.standard
        let keys = ["mello-tv.server.active-url", "mello-tv.server.known-endpoints", "mello-tv.server.id",
                    "mello-tv.server.tailscale-url", "family-stream.episode-watch-overrides.v1"]
        let previous = keys.map { defaults.object(forKey: $0) }
        defer { for (key, value) in zip(keys, previous) { if let value { defaults.set(value, forKey: key) } else { defaults.removeObject(forKey: key) } } }
        defaults.set("https://fixture.invalid", forKey: keys[0])
        defaults.set(["https://fixture.invalid"], forKey: keys[1])
        defaults.set("offline-fixture", forKey: keys[2])
        defaults.removeObject(forKey: keys[3])
        defaults.removeObject(forKey: keys[4])
        await fixture.configure(profilesFail: true)
        let profileStore = ProfileStore(api: api)
        await profileStore.loadProfiles()
        let profilePaths = await fixture.paths
        precondition(!profileStore.isLoading && profileStore.errorMessage != nil && profilePaths.count == 1,
                     "a reachable service failure returns after one load without discovery recursion")
        print("PASS profile service failure returns a stable error without recursive reload")

        await fixture.configure(ambiguousCreation: true)
        let creationStore = ProfileStore(api: api)
        creationStore.profiles = profiles
        do {
            try await creationStore.saveProfile(name: "New fixture", color: "#446e88", existingID: nil)
            preconditionFailure("creation reply should be lost")
        } catch { }
        try await creationStore.saveProfile(name: "New fixture", color: "#446e88", existingID: nil)
        let creations = await fixture.createCount
        precondition(creations == 1 && creationStore.activeProfile?.id == "created")
        print("PASS an ambiguous profile creation is reconciled before another POST")

        await fixture.configure(slowLibrary: true)
        let store = LibraryStore(api: api)
        let first = Task { await store.loadLibrary(for: "p") }
        await Task.yield()
        let second = Task { await store.loadLibrary(for: "p") }
        await first.value; await second.value
        let paths = await fixture.paths
        precondition(paths.filter { $0.hasSuffix("/library") }.count == 1 && store.library.list.count == 1)
        store.setEpisodeWatched(true, profileID: "p", title: title, season: 1, episode: 2)
        defaults.set("http://100.64.0.2:8080", forKey: keys[0])
        precondition(store.hasManualWatchedMark(profileID: "p", title: title, season: 1, episode: 2))
        store.setEpisodeWatched(false, profileID: "p", title: title, season: 1, episode: 2)
        precondition(!store.hasManualWatchedMark(profileID: "p", title: title, season: 1, episode: 2))
        print("PASS concurrent library reads share one request and local watched marks survive endpoint changes")

        await fixture.configure(qualityHeight: 360)
        let source = try await api.resolvePlayback(request: request, key: "quality-fixture")
        let height = await fixture.requestedHeight
        precondition(height == 360 && source.qualityLimit == 360 && source.url.host == "fixture.invalid")
        print("PASS profile quality ceiling reaches authorization and signed media retains the responding origin")

        let file = FileManager.default.temporaryDirectory.appending(path: "mellotv-progress-\(UUID()).json")
        defer { try? FileManager.default.removeItem(at: file) }
        func update(_ sequence: Int, _ position: Double) -> WatchProgressUpdate {
            WatchProgressUpdate(title: title, sessionId: "session", sequence: sequence, season: 0, episode: 0,
                episodeTitle: "", position: position, duration: 100, watchedSeconds: position)
        }
        await fixture.configure(progressStatus: 503)
        let journal = PlaybackProgressJournal(storageURL: file, api: api, household: { "fixture" })
        journal.enqueue(update(1, 25), request: request)
        await journal.flush().value
        precondition(journal.deferClose(sessionID: "session", profileID: "p"))
        let relaunched = PlaybackProgressJournal(storageURL: file, api: api, household: { "fixture" })
        precondition(relaunched.resumePosition(for: request) == 25)
        let otherHousehold = PlaybackProgressJournal(storageURL: file, api: api, household: { "other" })
        precondition(otherHousehold.resumePosition(for: request) == nil)
        await fixture.configure(progressStatus: 200)
        await relaunched.flush().value
        let deletes = await fixture.deletes
        let synced = PlaybackProgressJournal(storageURL: file, api: api, household: { "fixture" })
        precondition(synced.resumePosition(for: request) == nil && deletes == 1)
        print("PASS unsent measured progress survives relaunch, remains scoped, and revokes only after acknowledged save")

        await fixture.configure(progressStatus: 200, progressDelay: true)
        let changing = PlaybackProgressJournal(storageURL: file, api: api, household: { "fixture" })
        changing.enqueue(update(2, 30), request: request)
        let writer = changing.flush()
        try await Task.sleep(for: .milliseconds(50))
        changing.enqueue(update(3, 40), request: request)
        await writer.value
        let sequences = await fixture.progressSequences
        precondition(sequences == [2, 3] && changing.resumePosition(for: request) == nil,
                     "a stale acknowledgment must not delete a newer queued sample")
        await fixture.configure(progressStatus: 409)
        changing.enqueue(update(4, 50), request: request)
        await changing.flush().value
        precondition(changing.resumePosition(for: request) == 50 && !changing.deferClose(sessionID: "session", profileID: "p"))
        print("PASS newer samples survive in-flight acknowledgments and expired sessions preserve local resume without fabricated telemetry")
        print("All offline client reliability checks passed")
    }
}
