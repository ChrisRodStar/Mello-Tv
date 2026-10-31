import Foundation
import Network
import AVFoundation
import OSLog
import MelloKit
#if canImport(UIKit)
import UIKit
#endif

private let preparationLog = Logger(subsystem: Bundle.main.bundleIdentifier ?? "com.tinymellow.mellotv", category: "Preparation")

final class NetworkMonitor: @unchecked Sendable {
    static let shared = NetworkMonitor()
    private let monitor = NWPathMonitor()
    private let queue = DispatchQueue(label: "FamilyStream.NetworkMonitor", qos: .utility)
    private let lock = NSLock()
    private var _isExpensive: Bool = false
    private var _isConstrained: Bool = false
    private var _isConnected: Bool = false

    var isPreloadPermitted: Bool {
        lock.lock()
        defer { lock.unlock() }
        return _isConnected && !_isExpensive && !_isConstrained
    }

    private init() {
        monitor.pathUpdateHandler = { [weak self] path in
            guard let self else { return }
            self.lock.lock()
            self._isExpensive = path.isExpensive
            self._isConstrained = path.isConstrained
            self._isConnected = path.status == .satisfied
            self.lock.unlock()
            if path.status == .satisfied {
                Task { @MainActor in PlaybackProgressJournal.shared.flush() }
            }
        }
        monitor.start(queue: queue)
    }
}

// A reopened player must not read resume state ahead of its predecessor's
// final progress write. Entries remove themselves only while still the owner.
@MainActor
final class PlaybackWriteBarrier {
    private static var writes: [String: (UUID, Task<Void, Never>)] = [:]
    private static var versions: [String: UUID] = [:]

    static func version(for request: PlaybackRequest) -> UUID {
        let key = request.progressKey
        if let version = versions[key] { return version }
        if versions.count >= 256, let oldest = versions.keys.first(where: { writes[$0] == nil }) {
            versions.removeValue(forKey: oldest)
        }
        let version = UUID()
        if versions.count < 256 { versions[key] = version }
        return version
    }

    static func register(_ task: Task<Void, Never>, for request: PlaybackRequest) {
        let key = request.progressKey
        versions.removeValue(forKey: key)
        _ = version(for: request)
        let owner = UUID()
        writes[key] = (owner, task)
        Task {
            await task.value
            if writes[key]?.0 == owner { writes.removeValue(forKey: key) }
        }
    }

    static func settle(for request: PlaybackRequest) async {
        while let (_, task) = writes[request.progressKey] {
            await task.value
            if Task.isCancelled { return }
            // Allow the owned removal to run; a newer write is awaited too.
            await Task.yield()
        }
    }
}

// Authorization and its optional player always transfer as one owned value.
@MainActor
struct PreparedPlayback {
    let source: AuthorizedPlayback
    let player: AVPlayer?
    let warmup: PlaybackWarmup?
    let preparationTiming: PlaybackTiming?
    let progressVersion: UUID?
    let preparedPosition: Double?

    init(source: AuthorizedPlayback, player: AVPlayer?, warmup: PlaybackWarmup? = nil, preparationTiming: PlaybackTiming? = nil,
         progressVersion: UUID? = nil, preparedPosition: Double? = nil) {
        self.source = source
        self.player = player
        self.warmup = warmup
        self.preparationTiming = preparationTiming
        self.progressVersion = progressVersion
        self.preparedPosition = preparedPosition
    }
}

// One paused player owns readiness/preroll observation until transfer, discard
// or an eight-second bound. Its owned seek warms the intended resume position;
// claim/discard settles or cancels that seek before another owner may reposition.
@MainActor
final class PlaybackWarmup {
    private let player: AVPlayer
    private let timing: PlaybackTiming?
    private let targetPosition: Double
    private var seekTask: Task<Void, Never>?
    private var observation: NSKeyValueObservation?
    private var timeout: Timer?
    private var startedPreroll = false
    private var cancelled = false
    private var itemObservation: NSKeyValueObservation?
    private let started = ProcessInfo.processInfo.systemUptime

    init(player: AVPlayer, position: Double = 0, timing: PlaybackTiming? = nil) {
        self.player = player
        self.timing = timing
        self.targetPosition = position.isFinite ? max(0, position) : 0
        observation = player.observe(\.status, options: [.initial, .new]) { [weak self] _, _ in
            Task { @MainActor [weak self] in self?.ready() }
        }
        itemObservation = player.currentItem?.observe(\.status, options: [.initial, .new]) { [weak self] _, _ in
            Task { @MainActor [weak self] in self?.ready() }
        }
        timeout = Timer.scheduledTimer(withTimeInterval: 8, repeats: false) { [weak self] _ in
            Task { @MainActor [weak self] in self?.cancel() }
        }
    }

    private func ready() {
        guard !cancelled, !startedPreroll, player.status == .readyToPlay, player.currentItem?.status == .readyToPlay else { return }
        timing?.record(.prerollItemReady)
        startedPreroll = true
        seekTask = Task { @MainActor [weak self] in
            guard let self else { return }
            if targetPosition > 0 {
                timing?.record(.warmupSeekStarted)
                let deadline = Task { @MainActor [weak self] in
                    do { try await Task.sleep(for: .seconds(8)) } catch { return }
                    self?.player.currentItem?.cancelPendingSeeks()
                }
                defer { deadline.cancel() }
                _ = await player.seek(to: CMTime(seconds: targetPosition, preferredTimescale: 600),
                                      toleranceBefore: .positiveInfinity, toleranceAfter: .positiveInfinity)
                timing?.record(.warmupSeekFinished)
            }
            guard !cancelled, !Task.isCancelled else { return }
            timing?.record(.prerollStarted)
            player.preroll(atRate: 1) { [weak self] finished in
                Task { @MainActor [weak self] in
                    guard let self, !self.cancelled else { return }
                    self.timing?.record(.prerollFinished, flag: finished)
                    preparationLog.info("Preroll finished: \(finished, privacy: .public); elapsed: \(ProcessInfo.processInfo.systemUptime - self.started, privacy: .public) seconds")
                }
            }
        }
    }

    func claim(at position: Double) async -> Bool {
        cancelled = true
        observation = nil
        itemObservation = nil
        timeout?.invalidate()
        timeout = nil
        player.cancelPendingPrerolls()
        let pending = seekTask
        if abs(position - targetPosition) > 1 {
            pending?.cancel()
            player.currentItem?.cancelPendingSeeks()
        }
        await withTaskCancellationHandler {
            await pending?.value
        } onCancel: { [player] in
            pending?.cancel()
            Task { @MainActor in player.currentItem?.cancelPendingSeeks() }
        }
        seekTask = nil
        timing?.record(.warmupStopped)
        let current = player.currentTime().seconds
        return !Task.isCancelled && current.isFinite && abs(current - position) <= 1
    }

    func stopAndSettle() async {
        cancel()
        await seekTask?.value
        seekTask = nil
    }

    func cancel() {
        timing?.record(.warmupStopped)
        cancelled = true
        observation = nil
        itemObservation = nil
        timeout?.invalidate()
        timeout = nil
        seekTask?.cancel()
        player.currentItem?.cancelPendingSeeks()
        player.cancelPendingPrerolls()
    }
}

@MainActor
final class PreparedPlaybackService {
    static let shared = PreparedPlaybackService()
    private let api: MelloTVAPI
    private var speculative: Entry?
    private let permitsPreload: () -> Bool
    private var unresolvedCreate: (requestKey: String, key: String)?

    private final class Entry {
        let request: PlaybackRequest
        let task: Task<PreparedPlayback, Error>
        let key: String

        init(request: PlaybackRequest, key: String, task: Task<PreparedPlayback, Error>) {
            self.request = request
            self.key = key
            self.task = task
        }
    }

    init(api: MelloTVAPI = MelloTVAPI(baseURL: ServerAddress.baseURL),
         permitsPreload: @escaping () -> Bool = { NetworkMonitor.shared.isPreloadPermitted }) {
        self.api = api
        self.permitsPreload = permitsPreload
    }

    func preload(request: PlaybackRequest) {
        guard permitsPreload() else { return }
        guard speculative?.request.preparationKey != request.preparationKey else { return }
        clearAll()
        let request = request.traced(.preparation)
        request.timing?.record(.preparationStarted)
        let api = api
        let key = unresolvedCreate.flatMap { $0.requestKey == request.preparationKey ? $0.key : nil }
            ?? UUID().uuidString.lowercased()
        let task = Task {
            request.timing?.record(.writeBarrierStarted)
            await PlaybackWriteBarrier.settle(for: request)
            request.timing?.record(.writeBarrierFinished)
            try Task.checkCancellation()
            let progressVersion = PlaybackWriteBarrier.version(for: request)
            let source = try await api.resolvePlayback(request: request, key: key)
            // The returned warmup owns all asynchronous positioning. It does
            // not suspend this authorization-to-player ownership transfer.
            let item = AVPlayerItem(url: source.url)
            (PlaybackQuality(rawValue: source.qualityLimit) ?? .automatic).apply(to: item)
            item.canUseNetworkResourcesForLiveStreamingWhilePaused = true
            let player = AVPlayer(playerItem: item)
            player.pause()
            request.timing?.record(.preparationReady)
            return PreparedPlayback(source: source, player: player, warmup: PlaybackWarmup(player: player, position: source.resumeSeconds, timing: request.timing), preparationTiming: request.timing,
                                    progressVersion: progressVersion)
        }
        speculative = Entry(request: request, key: key, task: task)
    }

    func invalidate(request: PlaybackRequest) {
        guard speculative?.request.preparationKey == request.preparationKey else { return }
        clearAll()
    }

    func clearAll() {
        guard let entry = speculative else { return }
        speculative = nil
        discard(entry)
    }

    private func discard(_ entry: Entry) {
        entry.task.cancel()
        let api = api
        // If cancellation loses a race with creation, revoke the known unused
        // session after the task settles. It can never publish into the cache.
        Task {
            guard let prepared = try? await entry.task.value else { return }
            await prepared.warmup?.stopAndSettle()
            prepared.player?.pause()
            prepared.player?.replaceCurrentItem(with: nil)
            try? await api.revokePlayback(prepared.source, request: entry.request)
            if unresolvedCreate?.key == entry.key { unresolvedCreate = nil }
        }
    }

    func resolve(request: PlaybackRequest, idempotencyKey: String? = nil) async throws -> PreparedPlayback {
        if let entry = speculative, entry.request.preparationKey == request.preparationKey {
            request.timing?.record(.preparationClaimStarted, related: entry.request.timing)
            // Transfer before suspending. No other caller or completion can
            // claim this entry, and invalidation cannot cancel its new owner.
            speculative = nil
            unresolvedCreate = (request.preparationKey, entry.key)
            do {
                let prepared = try await withTaskCancellationHandler {
                    try await entry.task.value
                } onCancel: {
                    entry.task.cancel()
                }
                try Task.checkCancellation()
                await PlaybackWriteBarrier.settle(for: request)
                try Task.checkCancellation()
                if prepared.source.expiresAt.timeIntervalSinceNow > 90,
                   prepared.player?.currentItem?.status != .failed,
                   prepared.progressVersion == PlaybackWriteBarrier.version(for: request) {
                    unresolvedCreate = nil
                    let target = request.startFromBeginning ? 0 : prepared.source.resumeSeconds
                    let positioned = await prepared.warmup?.claim(at: target) ?? false
                    try Task.checkCancellation()
                    request.timing?.record(.preparationClaimed, related: prepared.preparationTiming,
                                           flag: prepared.player != nil,
                                           value: prepared.player?.currentItem?.status == .readyToPlay ? 1 : 0)
                    preparationLog.info("Claimed prepared player; item ready: \(prepared.player?.currentItem?.status == .readyToPlay, privacy: .public)")
                    return PreparedPlayback(source: prepared.source, player: prepared.player,
                                            preparationTiming: prepared.preparationTiming,
                                            progressVersion: prepared.progressVersion,
                                            preparedPosition: positioned ? target : nil)
                }
                unresolvedCreate = nil
                discard(entry)
            } catch {
                discard(entry)
                try Task.checkCancellation()
                // API retries already reconcile ambiguous creation using the
                // same key. Do not issue another create on an unknown outcome.
                throw error
            }
        } else {
            // Explicit Play supersedes a different speculative target.
            clearAll()
        }
        let key = unresolvedCreate.flatMap { $0.requestKey == request.preparationKey ? $0.key : nil }
            ?? idempotencyKey ?? UUID().uuidString.lowercased()
        unresolvedCreate = (request.preparationKey, key)
        request.timing?.record(.writeBarrierStarted)
        await PlaybackWriteBarrier.settle(for: request)
        request.timing?.record(.writeBarrierFinished)
        try Task.checkCancellation()
        let source: AuthorizedPlayback
        do {
            source = try await api.resolvePlayback(request: request, key: key)
        } catch {
            if case .service(410, _, _) = error as? APIError { unresolvedCreate = nil }
            throw error
        }
        if unresolvedCreate?.key == key { unresolvedCreate = nil }
        if Task.isCancelled {
            Task { try? await api.revokePlayback(source, request: request) }
            throw CancellationError()
        }
        return PreparedPlayback(source: source, player: nil)
    }
}

// Renewal and recovery share durable mutation ownership. An ambiguous reply is
// reconciled with the same operation/key before a different mutation is issued.
@MainActor
final class PlaybackSessionOperations {
    private let api: MelloTVAPI
    private struct Mutation {
        let source: AuthorizedPlayback
        let kind: String
        let key: String
    }
    private var pending: Mutation?
    private var latest: AuthorizedPlayback?

    init(api: MelloTVAPI = MelloTVAPI(baseURL: ServerAddress.baseURL)) {
        self.api = api
    }

    func renew(_ source: AuthorizedPlayback, request: PlaybackRequest) async throws -> AuthorizedPlayback {
        try await perform("renew", source: source, request: request)
    }

    func upgrade(_ source: AuthorizedPlayback, request: PlaybackRequest) async throws -> AuthorizedPlayback {
        try await perform("upgrade", source: source, request: request)
    }

    func recover(_ source: AuthorizedPlayback, request: PlaybackRequest) async throws -> AuthorizedPlayback {
        var current = latest.flatMap { $0.id == source.id ? $0 : nil } ?? source
        if let mutation = pending {
            do {
                current = try await perform(mutation.kind, source: mutation.source, request: request)
                if mutation.kind != "renew" { return current }
            } catch {
                guard (error as? APIError)?.requiresSourceRefresh == true else { throw error }
                return try await perform("refresh", source: current, request: request)
            }
        }
        do {
            return try await perform("backup", source: current, request: request)
        } catch {
            guard (error as? APIError)?.requiresSourceRefresh == true else { throw error }
            return try await perform("refresh", source: current, request: request)
        }
    }

    private func perform(_ kind: String, source: AuthorizedPlayback, request: PlaybackRequest) async throws -> AuthorizedPlayback {
        let mutation = pending ?? Mutation(source: source, kind: kind, key: UUID().uuidString.lowercased())
        pending = mutation
        do {
            let result = try await api.changePlayback(mutation.source, request: request,
                                                      kind: mutation.kind, key: mutation.key)
            latest = result
            pending = nil
            return result
        } catch {
            if let failure = error as? APIError,
               failure.requiresSourceRefresh || failure.isTerminalMutation
                    || (mutation.kind == "upgrade" && failure.isDefinitiveQualityUpgradeFailure) {
                pending = nil
            }
            throw error
        }
    }
}

// Owned by the full-screen presentation, not an episode view. It can transfer
// the one pending/ready preparation before the old episode is torn down.
@MainActor
final class NextEpisodePreparation {
    private let api = MelloTVAPI(baseURL: ServerAddress.baseURL)
    private var pending: PreparedNextEpisode?
    private var attemptedOwner: String?

    func prepare(after request: PlaybackRequest) {
        guard attemptedOwner != request.preparationKey else { return }
        cancel()
        attemptedOwner = request.preparationKey
        pending = PreparedNextEpisode(after: request, api: api)
    }

    func nextRequest(after request: PlaybackRequest) async throws -> PlaybackRequest? {
        if let pending, pending.ownerKey == request.preparationKey {
            do { return try await pending.nextRequest() }
            catch {
                if Task.isCancelled { throw CancellationError() }
                // A failed speculative metadata load must not prevent manual Next.
            }
        }
        return try await NextEpisodeResolver.next(after: request, api: api)
    }

    func claim(for request: PlaybackRequest) -> PreparedNextEpisode? {
        guard let pending, pending.matches(request) else {
            cancel()
            return nil
        }
        self.pending = nil
        return pending
    }

    func cancel() {
        pending?.cancel()
        pending = nil
        attemptedOwner = nil
    }
}

@MainActor
final class PreparedNextEpisode {
    let diagnosticTiming: PlaybackTiming
    let ownerKey: String
    let idempotencyKey = UUID().uuidString.lowercased()
    private let api: MelloTVAPI
    private let metadataTask: Task<PlaybackRequest?, Error>
    private var sourceTask: Task<AuthorizedPlayback?, Error>?
    private var request: PlaybackRequest?
    private var source: AuthorizedPlayback?
    private var abandoned = false
    private var expired = false
    var fallbackKey: String? { expired ? nil : idempotencyKey }

    init(after current: PlaybackRequest, api: MelloTVAPI) {
        self.api = api
        let trace = PlaybackTiming(action: .preparation, titleID: current.titleID, type: current.type)
        diagnosticTiming = trace
        trace.record(.preparationStarted)
        ownerKey = current.cacheKey
        metadataTask = Task {
            trace.record(.resumeMetadataStarted)
            let next = try await NextEpisodeResolver.next(after: current, api: api)
            trace.record(.resumeMetadataFinished, flag: next != nil)
            return next
        }
        sourceTask = Task {
            guard let next = try await metadataTask.value else { return nil }
            try Task.checkCancellation()
            request = next
            let resolved = try await api.resolvePlayback(request: next.traced(.preparation, using: trace), key: idempotencyKey)
            trace.record(.preparationReady)
            source = resolved
            guard !Task.isCancelled, !abandoned else {
                discardSource()
                throw CancellationError()
            }
            return resolved
        }
    }

    func nextRequest() async throws -> PlaybackRequest? {
        let next = try await metadataTask.value
        try Task.checkCancellation()
        request = next
        return next
    }

    func matches(_ next: PlaybackRequest) -> Bool {
        !abandoned && request?.cacheKey == next.cacheKey
    }

    func resolve(for next: PlaybackRequest) async throws -> AuthorizedPlayback? {
        guard matches(next), let sourceTask else { return nil }
        let resolved = try await withTaskCancellationHandler {
            try await sourceTask.value
        } onCancel: {
            sourceTask.cancel()
        }
        try Task.checkCancellation()
        guard let resolved else { return nil }
        // Leave headroom for initial buffering and the renewal owner's first tick.
        guard resolved.expiresAt.timeIntervalSinceNow > 90 else {
            expired = true
            discardSource()
            return nil
        }
        return resolved
    }

    func cancel() {
        abandoned = true
        metadataTask.cancel()
        sourceTask?.cancel()
        discardSource()
    }

    private func discardSource() {
        guard let source, let request else { return }
        self.source = nil
        let api = api
        // Best-effort, bounded revocation of a known unused session. If creation
        // was cancelled before its ID returned, the server lease bounds its life.
        Task {
            for attempt in 0..<3 {
                do {
                    try await api.revokePlayback(source, request: request)
                    return
                } catch {
                    guard attempt < 2 else { return }
                    do { try await Task.sleep(for: .seconds(attempt + 1)) } catch { return }
                }
            }
        }
    }
}

// Persist measured samples before sending them. A failed final save survives
// dismissal/relaunch; server sequence numbers make ambiguous retries idempotent.
@MainActor
final class PlaybackProgressJournal {
    static let shared = PlaybackProgressJournal()

    private struct Entry: Codable {
        let household: String
        let profileID: String
        let update: WatchProgressUpdate
        let savedAt: Date
        var closed = false
        var key: String { household + "/" + profileID + "/" + update.sessionId }
        var progressKey: String {
            "\(profileID)-\(update.title.type.rawValue)-\(update.title.tmdbID)" +
                (update.title.type == .tv ? "-s\(update.season)e\(update.episode)" : "")
        }
    }
    private struct State: Codable {
        var pending: [Entry] = []
        var checkpoints: [Entry] = []
    }
    private var state: State
    private let storageURL: URL
    private let api: MelloTVAPI
    private let household: () -> String
    private var writer: Task<Void, Never>?
    #if canImport(UIKit)
    private var backgroundTask: UIBackgroundTaskIdentifier = .invalid
    #endif

    init(storageURL: URL? = nil, api: MelloTVAPI = MelloTVAPI(baseURL: ServerAddress.baseURL),
         household: @escaping () -> String = { ServerAddress.serverIdentityURL.absoluteString }) {
        self.storageURL = storageURL ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appending(path: "MelloTV/playback-progress.json")
        self.api = api
        self.household = household
        state = (try? Data(contentsOf: self.storageURL)).flatMap { try? JSONDecoder().decode(State.self, from: $0) } ?? State()
    }

    func enqueue(_ update: WatchProgressUpdate, request: PlaybackRequest) {
        let entry = Entry(household: household(), profileID: request.profileID, update: update, savedAt: Date())
        if let existing = state.pending.first(where: { $0.key == entry.key }), existing.update.sequence >= update.sequence { return }
        state.pending.removeAll { $0.key == entry.key }
        state.pending.append(entry)
        state.checkpoints.removeAll { $0.household == entry.household && $0.progressKey == entry.progressKey }
        state.checkpoints.append(entry)
        persist()
    }

    func resumePosition(for request: PlaybackRequest) -> Double? {
        guard !request.startFromBeginning else { return nil }
        return state.checkpoints.last(where: {
            $0.household == household() && $0.progressKey == request.progressKey && $0.savedAt.timeIntervalSinceNow > -86_400
        })?.update.position
    }

    func overlay(_ library: ProfileLibrary, profileID: String) -> ProfileLibrary {
        var result = library
        for entry in state.checkpoints where entry.household == household() && entry.profileID == profileID
            && entry.savedAt.timeIntervalSinceNow > -86_400 {
            let update = entry.update
            result.progress.removeAll { $0.title.id == update.title.id && $0.season == update.season && $0.episode == update.episode }
            result.progress.append(WatchProgress(title: update.title, season: update.season, episode: update.episode,
                episodeTitle: update.episodeTitle, position: update.position, duration: update.duration,
                watchedSeconds: update.watchedSeconds, completed: false, updatedAt: entry.savedAt.timeIntervalSince1970))
        }
        result.progress.sort { $0.updatedAt > $1.updatedAt }
        return result
    }

    // Return true when revocation must wait for an unsent final sample.
    func deferClose(sessionID: String, profileID: String) -> Bool {
        guard let index = state.pending.firstIndex(where: {
            $0.household == household() && $0.profileID == profileID && $0.update.sessionId == sessionID
        }) else { return false }
        state.pending[index].closed = true
        persist()
        return true
    }

    @discardableResult
    func flush() -> Task<Void, Never> {
        if let writer { return writer }
        let task = Task { [self] in
            #if canImport(UIKit)
            backgroundTask = UIApplication.shared.beginBackgroundTask(withName: "Save playback progress") { [weak self] in
                Task { @MainActor in self?.writer?.cancel() }
            }
            #endif
            defer {
                writer = nil
                #if canImport(UIKit)
                if backgroundTask != .invalid {
                    UIApplication.shared.endBackgroundTask(backgroundTask)
                    backgroundTask = .invalid
                }
                #endif
            }
            var attempted = Set<String>()
            while !Task.isCancelled, let entry = state.pending.first(where: {
                $0.household == household() && !attempted.contains($0.key + "/\($0.update.sequence)")
            }) {
                attempted.insert(entry.key + "/\(entry.update.sequence)")
                var saved = false
                var terminal = false
                for attempt in 0..<3 {
                    do {
                        if attempt > 0 { try await Task.sleep(for: .seconds(attempt)) }
                        _ = try await api.updateProgress(profileID: entry.profileID, update: entry.update)
                        saved = true
                        break
                    } catch {
                        if Task.isCancelled { return }
                        if let failure = error as? APIError, failure.isExpiredPlayback {
                            terminal = true
                            break
                        }
                        if case .serverStatus(404) = error as? APIError {
                            terminal = true
                            break
                        }
                    }
                }
                guard saved || terminal else { continue }
                let closed = state.pending.first(where: { $0.key == entry.key })?.closed ?? entry.closed
                state.pending.removeAll { $0.key == entry.key && $0.update.sequence <= entry.update.sequence }
                if saved {
                    state.checkpoints.removeAll {
                        $0.key == entry.key && $0.update.sequence <= entry.update.sequence
                    }
                }
                // Expired sessions cannot accept late telemetry. Keep the local
                // position for resuming; never invent a replacement watch event.
                persist()
                if saved, closed, !state.pending.contains(where: { $0.key == entry.key }) {
                    try? await api.revokePlayback(id: entry.update.sessionId, profileID: entry.profileID)
                }
            }
        }
        writer = task
        return task
    }

    private func persist() {
        // Keep pending writes until acknowledged. Old position-only checkpoints
        // have a bounded lifetime and cannot overwrite new household state.
        state.checkpoints.removeAll { $0.savedAt.timeIntervalSinceNow < -86_400 }
        do {
            try FileManager.default.createDirectory(at: storageURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            let data = try JSONEncoder().encode(state)
            try data.write(to: storageURL, options: .atomic)
        } catch {
            preparationLog.error("Could not persist pending playback progress")
        }
    }
}

// Close leaves the final progress owner alive, then releases server-side lookup
// and relay resources. Jobs are bounded; server leases remain the offline fallback.
@MainActor
final class PlaybackSessionFinalizer {
    static let shared = PlaybackSessionFinalizer()
    private let api: MelloTVAPI
    private var jobs: [String: Task<Void, Never>] = [:]

    init(api: MelloTVAPI = MelloTVAPI(baseURL: ServerAddress.baseURL)) { self.api = api }

    func finish(_ source: AuthorizedPlayback, request: PlaybackRequest) {
        guard jobs[source.id] == nil, jobs.count < 32 else { return }
        let api = api
        jobs[source.id] = Task {
            await PlaybackWriteBarrier.settle(for: request)
            if !PlaybackProgressJournal.shared.deferClose(sessionID: source.id, profileID: request.profileID) {
                try? await api.revokePlayback(source, request: request)
            }
            jobs.removeValue(forKey: source.id)
        }
    }

    func settle() async {
        let pending = Array(jobs.values)
        for task in pending { await task.value }
    }
}
