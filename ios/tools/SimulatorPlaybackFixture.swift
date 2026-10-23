// Appended only to a disposable simulator build by prepare-simulator-fixture.py.
// This fixture never ships in the application's source target.
#if DEBUG && targetEnvironment(simulator)
final class SimulatorPlaybackFixture: URLProtocol, @unchecked Sendable {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var sessions: [String: String] = [:]
    nonisolated(unsafe) private static var progress: Double = 10
    nonisolated(unsafe) private static var targets: [String: [String: Any]] = [:]
    static func playbackURL(_ source: AuthorizedPlayback) -> URL {
        if ProcessInfo.processInfo.environment["FS_FIXTURE_FAIL_FIRST"] == "1", source.url.path.hasSuffix("/entry") {
            return Bundle.main.bundleURL.appendingPathComponent("intentionally-missing-fixture.mp4")
        }
        return Bundle.main.url(forResource: "playback-fixture", withExtension: "mp4")!
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        Self.lock.lock()
        defer { Self.lock.unlock() }
        let path = request.url!.path
        var requestData = request.httpBody ?? Data()
        if requestData.isEmpty, let stream = request.httpBodyStream {
            stream.open()
            defer { stream.close() }
            var buffer = [UInt8](repeating: 0, count: 4096)
            while stream.hasBytesAvailable {
                let read = stream.read(&buffer, maxLength: buffer.count)
                if read <= 0 { break }
                requestData.append(contentsOf: buffer.prefix(read))
            }
        }
        let body = (try? JSONSerialization.jsonObject(with: requestData)) as? [String: Any] ?? [:]
        let title: [String: Any] = ["id": 1622, "type": "tv", "title": "Supernatural",
            "overview": "Offline synthetic playback fixture. No household data or upstream video.",
            "rating": 8.3, "details": true, "genres": ["Drama"],
            "seasons": [["season": 1, "name": "Season 1", "episode_count": 2]]]
        let second: [String: Any] = ["id": 7, "type": "movie", "title": "Switch title fixture",
            "overview": "A second synthetic title for switching checks.", "rating": 7, "details": true]
        var result: Any = [:]
        var status = 200
        if path.hasSuffix("/profiles") {
            result = ["profiles": [["id": "simulator-profile", "name": "Simulator", "color": "#446e88",
                "preferences": ["completed": true, "genres": [], "format": "both", "liked_titles": []]]]]
        } else if path.hasSuffix("/favorites") || path.hasSuffix("/library") {
            result = ["titles": [Any]()]
        } else if path.hasSuffix("/browse") {
            result = ["records": [["data": ["id": "fixture", "heading": "Simulator fixtures", "titles": [title, second]]]]]
        } else if path.hasSuffix("/recommendations") {
            result = ["data": ["sections": [Any]()]]
        } else if path.hasSuffix("/history") {
            result = ["offset": 0, "limit": 100, "records": [["type": "tv", "id": 1622,
                "season": 1, "episode": 1, "position_seconds": Self.progress,
                "duration_seconds": 60, "watched_seconds": 10, "completed": false,
                "updated_at": "2026-10-19T12:00:00Z"]]]
        } else if path.contains("/titles/") {
            result = ["data": path.hasSuffix("/7") ? second : title]
        } else if path.contains("/seasons/") {
            result = ["data": ["show_id": 1622, "season": 1, "episodes": [
                ["episode": 1, "name": "Synthetic Episode One", "overview": "Offline sample", "runtime": 1],
                ["episode": 2, "name": "Synthetic Episode Two", "overview": "Offline sample", "runtime": 1]]]]
        } else if path.hasSuffix("/resume") {
            result = ["type": "tv", "id": 1622, "season": 1, "episode": 1,
                "position_seconds": Self.progress, "duration_seconds": 60, "watched_seconds": 10,
                "completed": false, "updated_at": "2026-10-19T12:00:00Z"]
        } else if path.contains("/playback") && request.httpMethod != "DELETE" {
            let key = request.value(forHTTPHeaderField: "Idempotency-Key") ?? "fixture"
            let session = Self.sessions[key] ?? "sim-session-\(Self.sessions.count + 1)"
            Self.sessions[key] = session
            var target: [String: Any] = ["type": body["type"] ?? "tv", "id": body["id"] ?? 1622]
            if (target["type"] as? String) == "tv" {
                target["season"] = body["season"] ?? 1
                target["episode"] = body["episode"] ?? 1
            }
            let creating = path.hasSuffix("/playback")
            let playbackID = creating ? session : String(path.split(separator: "/").dropLast().last!)
            if creating { Self.targets[playbackID] = target }
            else { target = Self.targets[playbackID] ?? target }
            result = ["id": playbackID, "title": target,
                "url": creating ? "/stream/fixture/entry?st=synthetic" : "/stream/fixture/recovered?st=synthetic",
                "format": "mp4", "expires_at": "2099-01-01T00:00:00Z", "revision": 1,
                "resume_seconds": Self.progress]
            print("SIMULATOR_FIXTURE playback session \(session)")
        } else if path.hasSuffix("/progress") {
            Self.progress = body["position_seconds"] as? Double ?? Self.progress
            result = ["type": "tv", "id": 1622, "season": 1, "episode": 1,
                "position_seconds": Self.progress, "duration_seconds": 60, "watched_seconds": 10,
                "completed": false, "updated_at": "2026-10-19T12:00:00Z"]
        } else {
            status = 204
        }
        let data = (try? JSONSerialization.data(withJSONObject: result)) ?? Data()
        let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil,
                                       headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        if status != 204 { client?.urlProtocol(self, didLoad: data) }
        client?.urlProtocolDidFinishLoading(self)
    }
}
#endif
