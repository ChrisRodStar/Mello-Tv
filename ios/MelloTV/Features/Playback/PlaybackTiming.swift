import Foundation
import OSLog

// Diagnostic context only: never used for playback identity or authorization.
// Fixed stage/action vocabulary and numeric fields keep grants, URLs, profile
// identifiers, title names and error descriptions out of the export.
final class PlaybackTiming: @unchecked Sendable {
    enum Action: String, Codable {
        case play, resume, restart, nextEpisode, retry, presentation, preparation, launch
    }
    enum Stage: String, Codable {
        case began, appDidFinishLaunching, resumeMetadataStarted, resumeMetadataFinished
        case presentationStarted, preparationStarted, preparationReady, preparationClaimStarted, preparationClaimed
        case writeBarrierStarted, writeBarrierFinished, authorizationStarted, authorizationFinished, authorizationFailed
        case warmupSeekStarted, warmupSeekFinished
        case prerollItemReady, prerollStarted, prerollFinished, warmupStopped
        case playerCreated, resumeSeekStarted, resumeSeekFinished, playerAttached, surfaceAttached, playRequested
        case itemReady, waiting, playing, firstFrameReadyForDisplay, playbackAdvancing
        case renditionsOffered, qualityChanged, qualityLimitChanged, deliverySample
        case recoveryStarted, failed, cancelled, closed
    }
    struct Event: Codable {
        let schema: Int
        let traceID: UUID
        let relatedTraceID: UUID?
        let action: Action
        let stage: Stage
        let titleID: Int?
        let mediaType: String?
        let season: Int?
        let episode: Int?
        let elapsedMS: Double
        let uptimeMS: Double
        let unixMS: Double
        let flag: Bool?
        let value: Double?
        let width: Double?
        let height: Double?
    }

    let id = UUID()
    private let action: Action
    private let titleID: Int?
    private let mediaType: String?
    private let season: Int?
    private let episode: Int?
    private let clock: @Sendable () -> TimeInterval
    private let started: TimeInterval
    private let lock = NSLock()
    private var recorded: Set<Stage> = []
    private var recurring: [Stage: Int] = [:]
    private var lastQuality: (Double, Double)?

    init(action: Action, titleID: Int? = nil, type: MediaType? = nil,
         season: Int? = nil, episode: Int? = nil,
         clock: @escaping @Sendable () -> TimeInterval = { ProcessInfo.processInfo.systemUptime }) {
        self.action = action
        self.titleID = titleID
        self.mediaType = type?.rawValue
        self.season = season
        self.episode = episode
        self.clock = clock
        self.started = clock()
        record(.began)
    }

    @discardableResult
    func record(_ stage: Stage, related: PlaybackTiming? = nil, flag: Bool? = nil,
                value: Double? = nil, width: Double? = nil, height: Double? = nil) -> Event? {
        lock.lock()
        guard !recorded.contains(.closed) else { lock.unlock(); return nil }
        if [.qualityChanged, .qualityLimitChanged, .deliverySample].contains(stage) {
            guard recurring[stage, default: 0] < 24 else { lock.unlock(); return nil }
            if stage == .qualityChanged {
                guard let width, let height, width > 0, height > 0,
                      lastQuality?.0 != width || lastQuality?.1 != height else { lock.unlock(); return nil }
                lastQuality = (width, height)
            }
            recurring[stage, default: 0] += 1
        } else {
            guard recorded.insert(stage).inserted else { lock.unlock(); return nil }
        }
        let now = clock()
        let event = Event(schema: 1, traceID: id, relatedTraceID: related?.id, action: action,
                          stage: stage, titleID: titleID, mediaType: mediaType, season: season, episode: episode,
                          elapsedMS: max(0, now - started) * 1000, uptimeMS: now * 1000,
                          unixMS: Date().timeIntervalSince1970 * 1000, flag: flag,
                          value: value.flatMap { $0.isFinite ? $0 : nil },
                          width: width.flatMap { $0.isFinite ? $0 : nil },
                          height: height.flatMap { $0.isFinite ? $0 : nil })
        lock.unlock()
        PlaybackTimingLog.append(event)
        return event
    }
}

private enum PlaybackTimingLog {
    private static let logger = Logger(subsystem: Bundle.main.bundleIdentifier ?? "com.tinymellow.mellotv", category: "PlaybackTiming")
    private static let queue = DispatchQueue(label: "FamilyStream.PlaybackTiming", qos: .utility)

    static func append(_ event: PlaybackTiming.Event) {
        #if DEBUG
        queue.async {
            do {
                let encoder = JSONEncoder()
                encoder.outputFormatting = [.sortedKeys]
                var data = try encoder.encode(event)
                logger.info("\(String(decoding: data, as: UTF8.self), privacy: .public)")
                data.append(10)
                let files = FileManager.default
                let directory = try files.url(for: .cachesDirectory, in: .userDomainMask,
                                              appropriateFor: nil, create: true).appendingPathComponent("PlaybackTiming", isDirectory: true)
                try files.createDirectory(at: directory, withIntermediateDirectories: true)
                let path = directory.appendingPathComponent("events.jsonl")
                let previous = directory.appendingPathComponent("events.previous.jsonl")
                // Two files, at most 1 MiB each. No synchronous disk I/O on the
                // player/main actor, no per-frame logging and no remote upload.
                let size = (try? files.attributesOfItem(atPath: path.path)[.size] as? NSNumber)?.intValue ?? 0
                if size + data.count > 1_048_576 {
                    if files.fileExists(atPath: previous.path) { try files.removeItem(at: previous) }
                    try files.moveItem(at: path, to: previous)
                }
                if !files.fileExists(atPath: path.path) {
                    guard files.createFile(atPath: path.path, contents: nil,
                                           attributes: [.posixPermissions: 0o600]) else { return }
                }
                let handle = try FileHandle(forWritingTo: path)
                defer { try? handle.close() }
                try handle.seekToEnd()
                try handle.write(contentsOf: data)
            } catch {
                // A diagnostic failure must never change playback or retry it.
                logger.error("Playback timing export unavailable")
            }
        }
        #endif
    }
}
