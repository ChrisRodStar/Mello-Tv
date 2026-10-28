import Foundation
import AVFoundation
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
    var upgradeFailures = 0
    var upgradeNoOp = false
    var upgradeFailureStatus: Int?
    var upgradeFailureCode: String?
    var playbackFormat = "mp4"
    var playbackHeights: [Int] = []
    var lastCodecs: [String] = []
    var attempts: [(String, String)] = []
    var revisions: [Int] = []
    var delay: UInt64 = 0

    func configure(busy: Bool = false, refresh: Bool = false, delay: UInt64 = 0, backupFailures: Int = 0, createFailures: Int = 0,
                   format: String = "mp4", availableHeights: [Int] = [], upgradeFailures: Int = 0,
                   upgradeNoOp: Bool = false, upgradeFailureStatus: Int? = nil,
                   upgradeFailureCode: String? = nil) {
        traceHeaders = []
        creates = 0; deleted = 0; attempts = []; revisions = []
        self.busy = busy; backupNeedsRefresh = refresh; self.delay = delay
        self.backupFailures = backupFailures
        self.createFailures = createFailures
        self.upgradeFailures = upgradeFailures
        self.upgradeNoOp = upgradeNoOp
        self.upgradeFailureStatus = upgradeFailureStatus
        self.upgradeFailureCode = upgradeFailureCode
        playbackFormat = format
        playbackHeights = availableHeights
        lastCodecs = []
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
        if path.hasSuffix("/upgrade"), upgradeFailures > 0 {
            upgradeFailures -= 1
            return (502, Data(#"{"error":"private_response"}"#.utf8))
        }
        if path.hasSuffix("/upgrade"), let status = upgradeFailureStatus, let code = upgradeFailureCode {
            return (status, try! JSONSerialization.data(withJSONObject: ["error": code]))
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
        if path.hasSuffix("/playback"), let device = body["device"] as? [String: Any] {
            lastCodecs = device["codecs"] as? [String] ?? []
        }
        if let revision = body["expected_revision"] as? Int { revisions.append(revision) }
        if path.hasSuffix("/playback") { creates += 1 }
        let number = creates
        let pause = delay
        if pause > 0 { try? await Task.sleep(nanoseconds: pause) }
        let target: [String: Any] = ["type": "movie", "id": body["id"] as? Int ?? 7]
        let expiry = ISO8601DateFormatter().string(from: Date().addingTimeInterval(3600))
        let playbackID = path.hasSuffix("/playback") ? "session-\(number)" : String(path.split(separator: "/").dropLast().last!)
        let expectedRevision = body["expected_revision"] as? Int ?? 0
        let isUpgrade = path.hasSuffix("/upgrade")
        let revision = isUpgrade && upgradeNoOp ? expectedRevision : expectedRevision + 1
        let reply: [String: Any] = ["id": playbackID, "title": target,
            "url": "/stream/fixture/entry?st=synthetic", "format": playbackFormat, "available_heights": playbackHeights,
            "expires_at": expiry, "revision": revision, "resume_seconds": 10,
            "reload_required": isUpgrade && !upgradeNoOp]
        return (200, try! JSONSerialization.data(withJSONObject: reply))
    }
}

@main
struct PlaybackReliabilityChecks {
    @MainActor
    static func main() async throws {
        var seeks = PlaybackSeekQueue()
        let firstTarget = seeks.target(skipping: 10, from: 100, duration: 300)!
        let firstSeek = seeks.enqueue(target: firstTarget, tolerance: 0.5)!
        let secondTarget = seeks.target(skipping: 10, from: 100, duration: 300)!
        precondition(secondTarget == 120, "repeated skips accumulate from the pending target")
        precondition(seeks.enqueue(target: secondTarget, tolerance: 0.5) == nil,
                     "only one AVPlayer seek may run at a time")
        let thirdTarget = seeks.target(skipping: 10, from: 101, duration: 300)!
        precondition(thirdTarget == 130)
        _ = seeks.enqueue(target: thirdTarget, tolerance: 0.5)
        let nextSeek = seeks.complete(firstSeek.id).next!
        precondition(nextSeek.target == 130, "intermediate seeks coalesce into the latest target")
        precondition(!seeks.complete(firstSeek.id).accepted,
                     "an obsolete completion cannot clear a newer seek")
        precondition(seeks.complete(nextSeek.id).accepted && seeks.pendingTarget == nil)
        let cancelled = seeks.enqueue(target: 200, tolerance: 0)!
        seeks.cancel()
        precondition(!seeks.complete(cancelled.id).accepted && seeks.pendingTarget == nil)
        precondition(seeks.target(skipping: -10, from: 2, duration: 300) == 0)
        precondition(seeks.target(skipping: 10, from: 295, duration: 300) == 300)
        precondition(seeks.target(skipping: 10, from: .nan, duration: 300) == nil)

        var replacedItemSeeks = PlaybackSeekQueue()
        let obsoleteItemSeek = replacedItemSeeks.enqueue(target: 90, tolerance: 0)!
        replacedItemSeeks.cancel() // observeItem invalidates old item's seek queue
        let replacementItemSeek = replacedItemSeeks.enqueue(target: 210, tolerance: 0)!
        let obsoleteCompletion = replacedItemSeeks.complete(obsoleteItemSeek.id)
        precondition(!obsoleteCompletion.accepted && replacedItemSeeks.pendingTarget == 210,
                     "an old item completion cannot release or clear a replacement item's seek gate")
        precondition(replacedItemSeeks.complete(replacementItemSeek.id).accepted
                     && replacedItemSeeks.pendingTarget == nil)
        print("PASS skip bursts coalesce, accumulate, clamp, and ignore stale item completions")

        var seekTimeout = PlaybackSeekTimeoutState()
        let oldItemRequest = UUID()
        seekTimeout.begin(oldItemRequest, now: 10)
        precondition(!seekTimeout.expire(oldItemRequest, now: 17.9, isForeground: true))
        precondition(!seekTimeout.expire(oldItemRequest, now: 30, isForeground: false),
                     "background time cannot clear an active foreground seek")
        precondition(seekTimeout.expire(oldItemRequest, now: 30, isForeground: true),
                     "a seek with no completion is released after the bounded foreground deadline")
        let newItemRequest = UUID()
        seekTimeout.begin(newItemRequest, now: 31)
        precondition(!seekTimeout.complete(oldItemRequest)
                     && !seekTimeout.expire(oldItemRequest, now: 60, isForeground: true)
                     && seekTimeout.requestID == newItemRequest,
                     "a stale completion or timeout cannot clear a newer item's seek")
        precondition(seekTimeout.expire(newItemRequest, now: 39, isForeground: true))
        print("PASS seek timeout reconciles only its owned request and remains dormant in background")

        var stalledClock = PlaybackClockStallDetector()
        precondition(!stalledClock.observe(position: 10, now: 0, expectsPlayback: true,
                                           isForeground: true, isSeeking: false))
        precondition(!stalledClock.observe(position: 10, now: 14.9, expectsPlayback: true,
                                           isForeground: true, isSeeking: false))
        precondition(!stalledClock.observe(position: 10.5, now: 15, expectsPlayback: true,
                                           isForeground: true, isSeeking: false),
                     "real progress resets the clock-stall deadline")
        precondition(!stalledClock.observe(position: 10.5, now: 40, expectsPlayback: false,
                                           isForeground: true, isSeeking: false))
        precondition(!stalledClock.observe(position: 10.5, now: 50, expectsPlayback: true,
                                           isForeground: false, isSeeking: false))
        precondition(!stalledClock.observe(position: 10.5, now: 51, expectsPlayback: true,
                                           isForeground: true, isSeeking: true))
        precondition(!stalledClock.observe(position: 10.5, now: 52, expectsPlayback: true,
                                           isForeground: true, isSeeking: false))
        precondition(!stalledClock.observe(position: 10.5, now: 66.9, expectsPlayback: true,
                                           isForeground: true, isSeeking: false))
        precondition(stalledClock.observe(position: 10.5, now: 67, expectsPlayback: true,
                                          isForeground: true, isSeeking: false),
                     "only 15 seconds of active foreground non-seeking clock stall is actionable")
        print("PASS clock-stall recovery deadline ignores paused, seeking, and background intervals")

        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [FixtureProtocol.self]
        let api = MelloTVAPI(baseURL: URL(string: "https://fixture.invalid")!, session: URLSession(configuration: config))
        let title = MediaTitle(tmdbID: 7, type: .movie, title: "Synthetic fixture")
        let request = PlaybackRequest(titleID: 7, type: .movie, title: title.title,
                                      profileID: "fixture-profile", catalogTitle: title)
        let state = FixtureProtocol.state

        var hevcAPI = api
        hevcAPI.codecSupportOverride = ["hvc1": true, "hev1": true]
        _ = try await hevcAPI.resolvePlayback(request: request, key: "hevc-supported")
        let supportedCodecs = await state.lastCodecs
        precondition(supportedCodecs.contains("hev1") && supportedCodecs.contains("hvc1"),
                     "supported HEVC families must be advertised in the device request")
        var noHEVCAPI = api
        noHEVCAPI.codecSupportOverride = ["hvc1": false, "hev1": false]
        _ = try await noHEVCAPI.resolvePlayback(request: request, key: "hevc-unsupported")
        let unsupportedCodecs = await state.lastCodecs
        precondition(!unsupportedCodecs.contains("hev1") && !unsupportedCodecs.contains("hvc1"),
                     "unsupported HEVC families must not be advertised")
        print("PASS device playback request advertises only capability-approved HEVC families")

        await state.configure(format: "hls", availableHeights: [720])
        let limited = try await api.resolvePlayback(request: request, key: "verified-720")
        precondition(limited.format == "hls" && limited.availableHeights == [720], "iOS must use only the server's verified rungs")
        await state.configure(format: "hls", availableHeights: [1080, 720])
        let upgraded = try await api.resolvePlayback(request: request, key: "verified-1080")
        precondition(upgraded.availableHeights == [1080, 720], "the verified 1080 rung must reach iOS")
        print("PASS HLS quality choices follow exact server-verified heights")

        let automaticItem = AVPlayerItem(url: upgraded.url)
        PlaybackQuality.automatic.apply(to: automaticItem)
        let replacementAutomaticItem = AVPlayerItem(url: upgraded.url)
        PlaybackQuality.automatic.apply(to: replacementAutomaticItem)
        precondition(automaticItem.preferredMaximumResolution == .zero
                     && replacementAutomaticItem.preferredMaximumResolution == .zero,
                     "automatic HLS selection must remain unconstrained across item replacement")
        let manualItem = AVPlayerItem(url: upgraded.url)
        PlaybackQuality.hd.apply(to: manualItem)
        let replacementManualItem = AVPlayerItem(url: upgraded.url)
        PlaybackQuality.hd.apply(to: replacementManualItem)
        precondition(manualItem.preferredMaximumResolution.height == 720
                     && replacementManualItem.preferredMaximumResolution.height == 720,
                     "manual 720p ceiling must survive item replacement")
        print("PASS automatic and manual quality policies persist across item replacement")
        await state.configure(format: "mp4")
        let upgradeSource = try await api.resolvePlayback(request: request, key: "upgrade-source")
        await state.configure(format: "mp4")
        let upgradeOperations = PlaybackSessionOperations(api: api)
        let upgradedSource = try await upgradeOperations.upgrade(upgradeSource, request: request)
        precondition(upgradedSource.reloadRequired && upgradedSource.format == "mp4"
                     && upgradedSource.availableHeights.isEmpty && upgradedSource.revision > upgradeSource.revision,
                     "MP4 upgrades must carry the rotated URL/revision even without HLS heights")
        print("PASS one-shot MP4 upgrade adopts reload requirement without HLS heights")
        await state.configure(format: "mp4", upgradeNoOp: true)
        let noUpgrade = try await upgradeOperations.upgrade(upgradedSource, request: request)
        precondition(!noUpgrade.reloadRequired && noUpgrade.revision == upgradedSource.revision,
                     "no-op upgrades must not request a replacement")
        print("PASS one-shot upgrade no-op preserves the current revision")
        await state.configure(format: "mp4", upgradeFailures: 3)
        let ambiguousUpgrade = PlaybackSessionOperations(api: api)
        do {
            _ = try await ambiguousUpgrade.upgrade(upgradedSource, request: request)
            preconditionFailure("fixture should exhaust upgrade transport retries")
        } catch { }
        let replayedUpgrade = try await ambiguousUpgrade.upgrade(upgradedSource, request: request)
        let upgradeAttempts = await state.attempts
        precondition(replayedUpgrade.reloadRequired && upgradeAttempts.count == 4
                     && upgradeAttempts.allSatisfy({ $0.0.hasSuffix("/upgrade") })
                     && Set(upgradeAttempts.map(\.1)).count == 1,
                     "ambiguous upgrade replay must keep one operation key")
        print("PASS ambiguous quality upgrade replays with the same operation key")
        for (status, code) in [(422, "no_source"), (422, "quality_unavailable"), (409, "active_quality_unknown")] {
            await state.configure(format: "mp4", upgradeFailureStatus: status, upgradeFailureCode: code)
            let rejectedUpgrade = PlaybackSessionOperations(api: api)
            do {
                _ = try await rejectedUpgrade.upgrade(replayedUpgrade, request: request)
                preconditionFailure("fixture should reject upgrade with \(code)")
            } catch { }
            let failedUpgradeAttempts = await state.attempts
            precondition(failedUpgradeAttempts.count == 1 && failedUpgradeAttempts[0].0.hasSuffix("/upgrade"),
                         "fixture must first reject the upgrade with \(code)")
            await state.configure(format: "mp4")
            _ = try await rejectedUpgrade.recover(replayedUpgrade, request: request)
            let recoveryAttempts = await state.attempts
            precondition(recoveryAttempts.count == 1 && recoveryAttempts[0].0.hasSuffix("/backup"),
                         "definitive \(code) rejection must clear the failed upgrade before recovery")
        }
        print("PASS definitive upgrade failures clear pending state so recovery can use backup")

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
