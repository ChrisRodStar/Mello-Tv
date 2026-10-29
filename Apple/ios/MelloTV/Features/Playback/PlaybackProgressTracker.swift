import Foundation
import AVFoundation
import MelloKit

@MainActor
final class PlaybackProgressTracker {
    private let api = MelloTVAPI(baseURL: ServerAddress.baseURL)
    private let request: PlaybackRequest
    private weak var player: AVPlayer?
    private let sessionID: String
    private let resumePosition: Double
    private var sequence = 0
    private var totalWatchedSeconds: Double = 0
    private var lastClockTime = Date()
    private var lastPosition: Double = 0
    private var lastSavedPosition: Double = -1
    private var lastSentSeconds: Double = 0
    private var timer: Timer?
    private var hasResumed = false
    private var isSeeking = false
    private var pendingUpdate: WatchProgressUpdate?
    private var saveTask: Task<Void, Never>?

    init(player: AVPlayer, request: PlaybackRequest, source: AuthorizedPlayback) {
        self.player = player
        self.request = request
        self.sessionID = source.id
        self.resumePosition = request.startFromBeginning ? 0 : source.resumeSeconds
    }

    func start() {
        timer?.invalidate()
        lastClockTime = Date()
        handlePlaybackItemReady()
        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in self?.sample() }
        }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
        sample()
        flush()
    }

    func setSeeking(_ seeking: Bool) {
        if seeking { sample() }
        isSeeking = seeking
        if !seeking, let player {
            lastPosition = player.currentTime().seconds
            lastClockTime = Date()
        }
    }

    func handlePlaybackItemReady() {
        guard !hasResumed, let player else { return }
        let duration = player.currentItem?.duration.seconds ?? 0
        guard duration.isFinite, duration > 0 else { return }
        hasResumed = true
        if resumePosition > 0, resumePosition < duration - 5 {
            let current = player.currentTime().seconds
            if !current.isFinite || abs(current - resumePosition) > 3.0 {
                player.seek(
                    to: CMTime(seconds: resumePosition, preferredTimescale: 600),
                    toleranceBefore: .positiveInfinity,
                    toleranceAfter: .positiveInfinity
                ) { _ in }
            }
            lastPosition = resumePosition
            lastClockTime = Date()
        }
    }

    func sample() {
        guard let player, let item = player.currentItem else { return }
        let now = Date()
        let elapsed = min(2.5, max(0, now.timeIntervalSince(lastClockTime)))
        let position = item.currentTime().seconds
        guard position.isFinite else { lastClockTime = now; return }
        let current = max(0, position)
        let rate = Double(player.rate)
        if player.timeControlStatus == .playing, !isSeeking, item.status == .readyToPlay, current > 0 {
            let advance = current - lastPosition
            if advance > 0, advance <= elapsed * max(1, rate) + 1 {
                totalWatchedSeconds += min(elapsed, advance / max(0.1, rate))
            }
        }
        lastPosition = current
        lastClockTime = now
        if totalWatchedSeconds - lastSentSeconds >= 15 { flush() }
    }

    func flush() {
        guard let item = player?.currentItem else { return }
        let rawPosition = item.currentTime().seconds
        let duration = item.duration.seconds
        guard rawPosition.isFinite, duration.isFinite, duration > 0 else { return }
        let position = min(duration, max(0, rawPosition))
        guard totalWatchedSeconds > 0 || position >= 5 else { return }
        guard totalWatchedSeconds > lastSentSeconds || abs(position - lastSavedPosition) >= 1 else { return }
        sequence += 1
        pendingUpdate = WatchProgressUpdate(title: request.catalogTitle, sessionId: sessionID, sequence: sequence,
            season: request.type == .tv ? (request.season ?? 0) : 0,
            episode: request.type == .tv ? (request.episode ?? 0) : 0,
            episodeTitle: request.episodeTitle ?? "", position: position, duration: duration,
            watchedSeconds: totalWatchedSeconds)
        lastSentSeconds = totalWatchedSeconds
        lastSavedPosition = position
        guard saveTask == nil else { return }
        // One owned writer serializes cumulative counters. Coalescing pending
        // samples is safe: a later sequence includes all earlier measured time.
        saveTask = Task { [self] in
            defer { saveTask = nil }
            while let update = pendingUpdate {
                pendingUpdate = nil
                do {
                    _ = try await api.updateProgress(profileID: request.profileID, update: update)
                } catch {
                    if pendingUpdate == nil {
                        lastSentSeconds = -1
                        lastSavedPosition = -1
                        // Retain the final sample through a short bounded retry,
                        // including view dismissal; no password/source data logs.
                        for attempt in 0..<2 {
                            do {
                                try await Task.sleep(for: .seconds(attempt + 1))
                                _ = try await api.updateProgress(profileID: request.profileID, update: update)
                                lastSentSeconds = update.watchedSeconds
                                lastSavedPosition = update.position
                                break
                            } catch { }
                        }
                    }
                }
            }
        }
        if let saveTask { PlaybackWriteBarrier.register(saveTask, for: request) }
    }
}
