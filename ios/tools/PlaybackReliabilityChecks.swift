import Foundation
import SwiftUI

// Compile this harness with the production API, preparation service and models.
// Every HTTP request is intercepted: no household server or provider is used.
final class FixtureProtocol: URLProtocol, @unchecked Sendable {
    static let state = FixtureState()
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Task {
            let result = await Self.state.respond(request)
            guard let response = HTTPURLResponse(url: request.url!, statusCode: result.0,
                                                  httpVersion: nil, headerFields: nil) else { return }
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: result.1)
            client?.urlProtocolDidFinishLoading(self)
        }
    }
    override func stopLoading() {}
}

actor FixtureState {
    var traceHeaders: [String] = []
    var creates = 0
    var deleted = 0
    var busy = false
    var backupNeedsRefresh = false
    var backupFailures = 0
    var createFailures = 0
    var attempts: [(String, String)] = []
    var revisions: [Int] = []
    var delay: UInt64 = 0

    func configure(busy: Bool = false, refresh: Bool = false, delay: UInt64 = 0, backupFailures: Int = 0, createFailures: Int = 0) {
        traceHeaders = []
        creates = 0; deleted = 0; attempts = []; revisions = []
        self.busy = busy; backupNeedsRefresh = refresh; self.delay = delay
        self.backupFailures = backupFailures
        self.createFailures = createFailures
    }
    func respond(_ request: URLRequest) async -> (Int, Data) {
        if let trace = request.value(forHTTPHeaderField: "X-Playback-Trace-ID") { traceHeaders.append(trace) }
        let path = request.url!.path
        let key = request.value(forHTTPHeaderField: "Idempotency-Key") ?? ""
        attempts.append((path, key))
        if request.httpMethod == "DELETE" { deleted += 1; return (204, Data()) }
        if busy {
            busy = false
            return (429, Data(#"{"error":"playback_busy"}"#.utf8))
        }
        if path.hasSuffix("/playback"), createFailures > 0 {
            createFailures -= 1
            return (502, Data(#"{"error":"private_response"}"#.utf8))
        }
        if path.hasSuffix("/backup"), backupFailures > 0 {
            backupFailures -= 1
            return (502, Data(#"{"error":"private_response"}"#.utf8))
        }
        if path.hasSuffix("/backup"), backupNeedsRefresh {
            return (409, Data(#"{"error":"source_refresh_required"}"#.utf8))
        }
        var requestData = request.httpBody ?? Data()
        if requestData.isEmpty, let stream = request.httpBodyStream {
            stream.open()
            defer { stream.close() }
            var buffer = [UInt8](repeating: 0, count: 4096)
            while stream.hasBytesAvailable {
                let count = stream.read(&buffer, maxLength: buffer.count)
                if count <= 0 { break }
                requestData.append(contentsOf: buffer.prefix(count))
            }
        }
        let body = (try? JSONSerialization.jsonObject(with: requestData)) as? [String: Any] ?? [:]
        if let revision = body["expected_revision"] as? Int { revisions.append(revision) }
        if path.hasSuffix("/playback") { creates += 1 }
        let number = creates
        let pause = delay
        if pause > 0 { try? await Task.sleep(nanoseconds: pause) }
        let target: [String: Any] = ["type": "movie", "id": body["id"] as? Int ?? 7]
        let expiry = ISO8601DateFormatter().string(from: Date().addingTimeInterval(3600))
        let playbackID = path.hasSuffix("/playback") ? "session-\(number)" : String(path.split(separator: "/").dropLast().last!)
        let reply: [String: Any] = ["id": playbackID, "title": target,
            "url": "/stream/fixture/entry?st=synthetic", "format": "mp4", "expires_at": expiry,
            "revision": (body["expected_revision"] as? Int ?? 0) + 1, "resume_seconds": 10]
        return (200, try! JSONSerialization.data(withJSONObject: reply))
    }
}

@main
struct PlaybackReliabilityChecks {
    @MainActor
    static func main() async throws {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [FixtureProtocol.self]
        let api = FamilyStreamAPI(baseURL: URL(string: "https://fixture.invalid")!, session: URLSession(configuration: config))
        let title = MediaTitle(tmdbID: 7, type: .movie, title: "Synthetic fixture")
        let request = PlaybackRequest(titleID: 7, type: .movie, title: title.title,
                                      profileID: "fixture-profile", catalogTitle: title)
        let state = FixtureProtocol.state

        await state.configure(busy: true)
        _ = try await api.resolvePlayback(request: request, key: "stable-create")
        let attempts = await state.attempts
        precondition(attempts.count == 2 && Set(attempts.map(\.1)) == ["stable-create"], "busy retry changed key")
        print("PASS busy setup retries with the same idempotency key")

        await state.configure(delay: 100_000_000)
        let service = PreparedPlaybackService(api: api, permitsPreload: { true })
        service.preload(request: request)
        let first = try await service.resolve(request: request)
        precondition(first.player != nil, "matching preload not transferred")
        let second = try await service.resolve(request: request)
        precondition(second.source.id != first.source.id && second.player == nil, "consumed preload resurrected")
        let creates = await state.creates
        precondition(creates == 2)
        first.player?.replaceCurrentItem(with: nil)
        print("PASS in-flight preparation transfers exactly once")

        await state.configure(delay: 20_000_000)
        let restart = PlaybackRequest(titleID: 7, type: .movie, title: title.title, profileID: "fixture-profile", catalogTitle: title, startFromBeginning: true)
        service.preload(request: request)
        let restarted = try await service.resolve(request: restart)
        precondition(restarted.player != nil, "restart discarded matching episode preparation")
        let restartCreates = await state.creates
        precondition(restartCreates == 1, "local seek choice created a duplicate session")
        restarted.player?.replaceCurrentItem(with: nil)
        print("PASS restart claims the matching prepared authorization")

        await state.configure(delay: 150_000_000)
        service.preload(request: request)
        try await Task.sleep(for: .milliseconds(20))
        service.clearAll()
        try await Task.sleep(for: .milliseconds(200))
        let fresh = try await service.resolve(request: request)
        precondition(fresh.player == nil, "cancelled preparation republished")
        print("PASS late cancelled completion cannot repopulate preparation")

        await state.configure(refresh: true)
        let operations = PlaybackSessionOperations(api: api)
        let recovered = try await operations.recover(second.source, request: request)
        precondition(recovered.id == second.source.id)
        let paths = await state.attempts.map(\.0)
        precondition(paths.count == 2 && paths[0].hasSuffix("/backup") && paths[1].hasSuffix("/refresh"))
        print("PASS exhausted backups trigger one fresh-source mutation")
        await state.configure(backupFailures: 3)
        let ambiguous = PlaybackSessionOperations(api: api)
        do {
            _ = try await ambiguous.recover(second.source, request: request)
            preconditionFailure("fixture should exhaust transport retries")
        } catch { }
        _ = try await ambiguous.recover(second.source, request: request)
        let retried = await state.attempts
        precondition(retried.count == 4 && Set(retried.map(\.1)).count == 1,
                     "ambiguous recovery changed operation key")
        print("PASS ambiguous recovery retains its mutation key across retry")

        await state.configure()
        let renewal = PlaybackSessionOperations(api: api)
        _ = try await renewal.renew(second.source, request: request)
        _ = try await renewal.recover(second.source, request: request)
        let coordinated = await state.attempts
        precondition(coordinated.count == 2 && coordinated[1].0.hasSuffix("/backup"))
        let revisions = await state.revisions
        precondition(revisions == [1, 2], "recovery used revision from before renewal")
        print("PASS recovery reconciles latest renewal before changing source")
        await state.configure()
        let finalSave = Task { @MainActor in try? await Task.sleep(for: .milliseconds(100)) }
        let writer = Task { @MainActor in _ = await finalSave.value }
        PlaybackWriteBarrier.register(writer, for: request)
        let reopening = Task { @MainActor in try await service.resolve(request: request) }
        try await Task.sleep(for: .milliseconds(20))
        let earlyCreates = await state.creates
        precondition(earlyCreates == 0, "reopen overtook final progress save")
        _ = try await reopening.value
        print("PASS reopening waits for the previous final progress save")
        await state.configure(createFailures: 3)
        let lostCreate = PreparedPlaybackService(api: api, permitsPreload: { true })
        do {
            _ = try await lostCreate.resolve(request: request)
            preconditionFailure("fixture should exhaust create retries")
        } catch { }
        _ = try await lostCreate.resolve(request: request)
        let createRetries = await state.attempts
        precondition(createRetries.count == 4 && Set(createRetries.map(\.1)).count == 1,
                     "ambiguous creation changed key after manual retry")
        print("PASS ambiguous setup retains its creation key across retry")
        await state.configure()
        let stalePreparation = PreparedPlaybackService(api: api, permitsPreload: { true })
        stalePreparation.preload(request: request)
        try await Task.sleep(for: .milliseconds(80))
        let finalWrite = Task { @MainActor in try? await Task.sleep(for: .milliseconds(50)) }
        let finalWriter = Task { @MainActor in _ = await finalWrite.value }
        PlaybackWriteBarrier.register(finalWriter, for: request)
        let afterSave = try await stalePreparation.resolve(request: request)
        precondition(afterSave.player == nil, "preparation from before final progress save was reused")
        let refreshedCreates = await state.creates
        precondition(refreshedCreates == 2, "stale resume authorization was not recreated")
        try await Task.sleep(for: .milliseconds(50))
        print("PASS final progress generation invalidates earlier resume preparation")

        await state.configure()
        let closingWrite = Task { @MainActor in try? await Task.sleep(for: .milliseconds(80)) }
        let closingWriter = Task { @MainActor in _ = await closingWrite.value }
        PlaybackWriteBarrier.register(closingWriter, for: request)
        let finalizer = PlaybackSessionFinalizer(api: api)
        finalizer.finish(afterSave.source, request: request)
        try await Task.sleep(for: .milliseconds(10))
        let earlyDeletes = await state.deleted
        precondition(earlyDeletes == 0, "session revoked before final progress writer settled")
        await finalizer.settle()
        let finalDeletes = await state.deleted
        precondition(finalDeletes == 1, "closed playback session was not revoked")
        print("PASS session close waits for final progress before releasing backend work")

        let clock = TimingFixtureClock()
        let timing = PlaybackTiming(action: .play, titleID: 7, type: .movie, clock: { clock.read() })
        clock.advance(0.25)
        let marker = timing.record(.firstFrameReadyForDisplay, value: .infinity, width: 1920, height: 1080)!
        precondition(abs(marker.elapsedMS - 250) < 0.001)
        precondition(marker.value == nil && timing.record(.firstFrameReadyForDisplay) == nil)
        let encodedMarker = String(decoding: try JSONEncoder().encode(marker), as: UTF8.self)
        precondition(!encodedMarker.contains("fixture-profile") && !encodedMarker.contains("Synthetic fixture") && !encodedMarker.contains("https://"))
        let traced = request.traced(.play, using: timing)
        precondition(traced == request && Set([traced, request]).count == 1 && traced.cacheKey == request.cacheKey)
        await state.configure(busy: true)
        _ = try await api.resolvePlayback(request: traced, key: "timed-create")
        let headers = await state.traceHeaders
        precondition(headers.count == 2 && Set(headers) == [timing.id.uuidString.lowercased()])
        precondition(timing.record(.authorizationFinished) == nil, "authorization milestone was not recorded")
        precondition(timing.record(.qualityChanged, width: 1280, height: 720) != nil)
        precondition(timing.record(.qualityChanged, width: 1280, height: 720) == nil)
        precondition(timing.record(.qualityChanged, width: 1920, height: 1080) != nil)
        for _ in 0..<24 { timing.record(.deliverySample, value: 1_000_000) }
        precondition(timing.record(.deliverySample, value: 1_000_000) == nil)
        timing.record(.closed)
        precondition(timing.record(.playing) == nil, "late player callback changed a closed attempt")
        print("PASS monotonic once-only timing, safe fields, stable request identity and retry trace correlation")
        print("All offline playback reliability checks passed")
    }
}

private final class TimingFixtureClock: @unchecked Sendable {
    private let lock = NSLock()
    private var value: TimeInterval = 100
    func read() -> TimeInterval { lock.lock(); defer { lock.unlock() }; return value }
    func advance(_ seconds: TimeInterval) { lock.lock(); defer { lock.unlock() }; value += seconds }
}
