import Foundation
import AVFoundation
import MelloKit

// Coalesce repeated skips while AVPlayer finishes its current seek. Requests
// accumulate from the user's latest target, not the player's stale position.
struct PlaybackSeekQueue {
    struct Request {
        let id = UUID()
        let target: Double
        let tolerance: Double
    }

    private(set) var pendingTarget: Double?
    private var active: Request?
    private var tolerance: Double = 0
    var activeRequest: Request? { active }

    func target(skipping delta: Double, from current: Double, duration: Double) -> Double? {
        let base = pendingTarget ?? current
        guard base.isFinite, delta.isFinite, duration.isFinite, duration > 0 else { return nil }
        return max(0, min(duration, base + delta))
    }

    mutating func enqueue(target: Double, tolerance: Double) -> Request? {
        pendingTarget = target
        self.tolerance = tolerance
        return beginNext()
    }

    mutating func complete(_ id: UUID) -> (accepted: Bool, next: Request?) {
        guard let active, active.id == id else { return (false, nil) }
        self.active = nil
        if pendingTarget == active.target {
            pendingTarget = nil
            return (true, nil)
        }
        return (true, beginNext())
    }

    mutating func cancel() {
        pendingTarget = nil
        active = nil
    }

    private mutating func beginNext() -> Request? {
        guard active == nil, let target = pendingTarget else { return nil }
        let request = Request(target: target, tolerance: tolerance)
        active = request
        return request
    }
}

/// Deadline ownership for a seek completion. A replacement item or a newer
/// seek invalidates the old request ID, so a delayed callback cannot clear the
/// current seek gate.
struct PlaybackSeekTimeoutState: Equatable {
    static let timeout: TimeInterval = 8

    private(set) var requestID: UUID?
    private(set) var startedAt: TimeInterval?

    mutating func begin(_ requestID: UUID, now: TimeInterval) {
        guard now.isFinite else { return }
        if self.requestID == requestID { return }
        self.requestID = requestID
        startedAt = now
    }

    mutating func complete(_ requestID: UUID) -> Bool {
        guard self.requestID == requestID else { return false }
        reset()
        return true
    }

    mutating func expire(_ requestID: UUID, now: TimeInterval, isForeground: Bool) -> Bool {
        guard isForeground, self.requestID == requestID,
              let startedAt, now.isFinite, now - startedAt >= Self.timeout else { return false }
        reset()
        return true
    }

    mutating func reset() {
        requestID = nil
        startedAt = nil
    }
}

/// Detects a player clock that stops while playback is expected. Paused,
/// seeking, backgrounded, and buffering intervals continually re-arm the
/// baseline instead of counting as user-visible playback stalls.
struct PlaybackClockStallDetector: Equatable {
    static let timeout: TimeInterval = 15
    private(set) var lastPosition: Double?
    private(set) var lastProgressAt: TimeInterval?

    mutating func observe(position: Double, now: TimeInterval, expectsPlayback: Bool,
                          isForeground: Bool, isSeeking: Bool) -> Bool {
        guard position.isFinite, now.isFinite else { return false }
        guard expectsPlayback, isForeground, !isSeeking else {
            reset()
            return false
        }
        guard let previousPosition = lastPosition, let lastProgressAt else {
            lastPosition = position
            self.lastProgressAt = now
            return false
        }
        let delta = position - previousPosition
        if delta > 0.05 || delta < -0.25 {
            lastPosition = position
            self.lastProgressAt = now
            return false
        }
        lastPosition = position
        return now - lastProgressAt >= Self.timeout
    }

    mutating func reset() {
        lastPosition = nil
        lastProgressAt = nil
    }
}

/// Episode-scoped state for the near-end prompt. The view owns clocks and
/// playback observations; this value owns the rules so seeks/backgrounding and
/// episode replacement can be tested without AVPlayer or network access.
struct EpisodePromptState: Equatable {
    static let countdownLength = 10

    private(set) var ownerKey: String?
    private(set) var isVisible = false
    private(set) var isCountingDown = false
    private(set) var secondsRemaining = countdownLength
    private(set) var isCancelled = false
    private(set) var isSuppressed = false
    private(set) var hasNaturallyEarnedCountdown = false
    private var suppressionPosition: Double?

    mutating func bind(to key: String) {
        guard ownerKey != key else { return }
        ownerKey = key
        reset()
    }

    mutating func consider(position: Double, cardTrigger: Double, countdownTrigger: Double,
                           naturallyAdvanced: Bool) {
        guard position.isFinite, cardTrigger.isFinite, countdownTrigger.isFinite else { return }
        if isSuppressed {
            if naturallyAdvanced, let suppressionPosition, position > suppressionPosition + 1 {
                isSuppressed = false
                self.suppressionPosition = nil
            } else {
                return
            }
        }
        guard !isCancelled else { return }
        if position >= cardTrigger {
            isVisible = true
        } else if isVisible {
            hideAndReset()
        }
        if isVisible, position >= countdownTrigger, naturallyAdvanced {
            isCountingDown = true
            hasNaturallyEarnedCountdown = true
        }
    }

    mutating func seekOrScrub(to position: Double) {
        isSuppressed = true
        suppressionPosition = position.isFinite ? max(0, position) : nil
        hideAndReset()
    }

    mutating func backgrounded(at position: Double) {
        // Returning to the app must never auto-play a next episode based on an
        // unattended countdown. Require natural playback to re-arm the prompt.
        seekOrScrub(to: position)
    }

    mutating func keepWatching() {
        isCancelled = true
        hideAndReset()
    }

    /// Returns true exactly once when the visible countdown expires.
    mutating func tick() -> Bool {
        guard isVisible, isCountingDown, secondsRemaining > 0 else { return false }
        secondsRemaining -= 1
        if secondsRemaining == 0 {
            isCountingDown = false
            return true
        }
        return false
    }

    mutating func naturalEnd() {
        guard isVisible, !isCancelled, hasNaturallyEarnedCountdown else { return }
        if secondsRemaining > 0 { isCountingDown = true }
    }

    mutating func playNextNow() {
        isCancelled = true
        hideAndReset()
    }

    private mutating func reset() {
        isVisible = false
        isCountingDown = false
        secondsRemaining = Self.countdownLength
        isCancelled = false
        isSuppressed = false
        hasNaturallyEarnedCountdown = false
        suppressionPosition = nil
    }

    private mutating func hideAndReset() {
        isVisible = false
        isCountingDown = false
        secondsRemaining = Self.countdownLength
        hasNaturallyEarnedCountdown = false
    }
}

/// Bounded retry policy for speculative next-episode metadata. The caller
/// supplies monotonic time and resets this value when AVPlayer replaces items.
struct NextEpisodeLookupState: Equatable {
    private(set) var attempts = 0
    private(set) var isInFlight = false
    private(set) var hasSuccessor = false
    private(set) var hasNoSuccessor = false
    private(set) var retryAt: TimeInterval?

    static let maximumAttempts = 3

    mutating func beginAttempt(now: TimeInterval) -> Bool {
        guard now.isFinite, !isInFlight, !hasSuccessor, !hasNoSuccessor,
              attempts < Self.maximumAttempts,
              retryAt.map({ now >= $0 }) ?? true else { return false }
        attempts += 1
        isInFlight = true
        retryAt = nil
        return true
    }

    mutating func complete(foundSuccessor: Bool) {
        guard isInFlight else { return }
        isInFlight = false
        hasSuccessor = foundSuccessor
        hasNoSuccessor = !foundSuccessor
        retryAt = nil
    }

    /// Also used when backgrounding cancels the in-flight request. It only
    /// advances state once, so the task's later cancellation handler is safe.
    mutating func fail(now: TimeInterval) {
        guard isInFlight else { return }
        isInFlight = false
        guard attempts < Self.maximumAttempts else {
            retryAt = nil
            return
        }
        let delay: TimeInterval = attempts == 1 ? 5 : 15
        retryAt = now.isFinite ? now + delay : nil
    }
}

enum PlaybackQuality: Int, CaseIterable, Identifiable {
    case automatic = 0, fullHD = 1080, hd = 720, sd = 480, low = 360

    var id: Int { rawValue }
    var height: Int { rawValue }
    var label: String { self == .automatic ? "Auto" : "\(rawValue)p" }

    // Auto leaves HLS rendition choice with AVPlayer. Explicit choices remain
    // ceilings and must be reapplied whenever the authorized item is replaced.
    func apply(to item: AVPlayerItem) {
        item.preferredMaximumResolution = height == 0
            ? .zero
            : CGSize(width: height * 16 / 9, height: height)
        item.preferredPeakBitRate = 0
    }
}

struct PlaybackRequest: Hashable, Identifiable, Sendable {
    let titleID: Int
    let type: MediaType
    let title: String
    let profileID: String
    let season: Int?
    let episode: Int?
    let episodeTitle: String?
    let catalogTitle: MediaTitle
    let startFromBeginning: Bool
    var timing: PlaybackTiming?

    init(
        titleID: Int,
        type: MediaType,
        title: String,
        profileID: String,
        season: Int? = nil,
        episode: Int? = nil,
        episodeTitle: String? = nil,
        catalogTitle: MediaTitle,
        startFromBeginning: Bool = false,
        timing: PlaybackTiming? = nil
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
        self.timing = timing
    }

    // Timing is deliberately excluded from equality, hashing and cache keys.
    static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.titleID == rhs.titleID && lhs.type == rhs.type && lhs.title == rhs.title &&
        lhs.profileID == rhs.profileID && lhs.season == rhs.season && lhs.episode == rhs.episode &&
        lhs.episodeTitle == rhs.episodeTitle && lhs.catalogTitle == rhs.catalogTitle &&
        lhs.startFromBeginning == rhs.startFromBeginning
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(titleID); hasher.combine(type); hasher.combine(title)
        hasher.combine(profileID); hasher.combine(season); hasher.combine(episode)
        hasher.combine(episodeTitle); hasher.combine(catalogTitle); hasher.combine(startFromBeginning)
    }

    func traced(_ action: PlaybackTiming.Action, using trace: PlaybackTiming? = nil) -> Self {
        var copy = self
        copy.timing = trace ?? PlaybackTiming(action: action, titleID: titleID, type: type, season: season, episode: episode)
        return copy
    }

    var id: String { cacheKey }

    // Restart/resume is a local seek choice; both use the same authorization.
    var preparationKey: String { progressKey }

    var cacheKey: String { progressKey + (startFromBeginning ? "-from-start" : "") }

    var progressKey: String {
        if type == .tv {
            return "\(profileID)-\(type.rawValue)-\(titleID)-s\(season ?? 0)e\(episode ?? 0)"
        }
        return "\(profileID)-\(type.rawValue)-\(titleID)"
    }
}

// Shared by explicit Next Episode and near-end preparation. Metadata lookup does
// not resolve providers, create a playback session or fetch video bytes.
enum NextEpisodeResolver {
    static func next(after request: PlaybackRequest, api: MelloTVAPI) async throws -> PlaybackRequest? {
        guard request.type == .tv, let season = request.season, let episode = request.episode else { return nil }
        let episodes = try await api.seasonEpisodes(tvID: request.titleID, season: season)
        try Task.checkCancellation()
        if let next = episodes.sorted(by: { $0.number < $1.number }).first(where: { $0.number > episode }) {
            return makeRequest(next, season: season, after: request)
        }
        let details = try await api.details(for: request.catalogTitle)
        try Task.checkCancellation()
        guard let nextSeason = details.seasons.filter({ $0.number > season && $0.episodeCount > 0 })
            .sorted(by: { $0.number < $1.number }).first else { return nil }
        let nextEpisodes = try await api.seasonEpisodes(tvID: request.titleID, season: nextSeason.number)
        try Task.checkCancellation()
        guard let first = nextEpisodes.sorted(by: { $0.number < $1.number }).first else {
            throw APIError.serverMessage("The next season’s episodes aren’t available yet.")
        }
        return makeRequest(first, season: nextSeason.number, after: request)
    }

    private static func makeRequest(_ episode: Episode, season: Int, after request: PlaybackRequest) -> PlaybackRequest {
        PlaybackRequest(titleID: request.titleID, type: .tv, title: request.title,
            profileID: request.profileID, season: season, episode: episode.number,
            episodeTitle: episode.name, catalogTitle: request.catalogTitle)
    }
}
