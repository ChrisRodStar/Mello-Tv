import Foundation
import CoreFoundation
import MelloKit

enum PlaybackSegmentKind: String, Sendable {
    case intro
    case recap
    case credits
}

struct PlaybackSegment: Identifiable, Equatable, Sendable {
    let kind: PlaybackSegmentKind
    let start: Double
    let end: Double

    var openingSkipLabel: String? {
        switch kind {
        case .intro: "Skip Intro"
        case .recap: "Skip Recap"
        case .credits: nil
        }
    }

    var id: String { "\(kind.rawValue)-\(Int((start * 1_000).rounded()))-\(Int((end * 1_000).rounded()))" }
}

struct PlaybackSegments: Sendable {
    let duration: Double
    let segments: [PlaybackSegment]
    // Retain the source name so the UI can provide the attribution TheIntroDB requests.
    let source: String?

    static func empty(duration: Double) -> Self {
        Self(duration: duration.isFinite ? max(0, duration) : 0, segments: [], source: nil)
    }
}

struct PlaybackSegmentHTTPResponse: Sendable {
    let statusCode: Int
    let url: URL?
    let mimeType: String?
    let data: Data
}

actor PlaybackSegmentService {
    typealias Transport = @Sendable (URLRequest) async throws -> PlaybackSegmentHTTPResponse

    static let shared = PlaybackSegmentService()

    private struct Key: Hashable, Sendable {
        let tmdbID: Int
        let imdbID: String?
        let season: Int
        let episode: Int
        let durationMilliseconds: Int64
    }

    private struct CacheEntry {
        let value: PlaybackSegments
        let expiresAt: Date
        let accessOrder: UInt64
    }

    private enum FetchResult: Sendable {
        case success(PlaybackSegments)
        case failure
    }

    private let transport: Transport
    private var cache: [Key: CacheEntry] = [:]
    private var flights: [Key: Task<FetchResult, Never>] = [:]
    private var accessOrder: UInt64 = 0

    init(transport: Transport? = nil) {
        self.transport = transport ?? Self.makeDefaultTransport()
    }

    func segments(for request: PlaybackRequest, duration: Double) async -> PlaybackSegments {
        guard request.type == .tv,
              request.titleID > 0,
              let season = request.season, (0...10_000).contains(season),
              let episode = request.episode, (1...100_000).contains(episode),
              duration.isFinite, duration > 0,
              let durationMilliseconds = Self.milliseconds(duration) else {
            return .empty(duration: duration)
        }
        let imdbID = Self.validIMDbID(request.catalogTitle.imdbID)
        let key = Key(tmdbID: request.titleID, imdbID: imdbID, season: season, episode: episode,
                      durationMilliseconds: durationMilliseconds)

        let now = Date()
        if let entry = cache[key] {
            if entry.expiresAt > now {
                accessOrder &+= 1
                cache[key] = CacheEntry(value: entry.value, expiresAt: entry.expiresAt,
                                        accessOrder: accessOrder)
                return entry.value
            }
            cache.removeValue(forKey: key)
        }

        let task: Task<FetchResult, Never>
        if let existing = flights[key] {
            task = existing
        } else {
            guard flights.count < 4 else { return .empty(duration: duration) }
            let transport = self.transport
            task = Task.detached(priority: .utility) {
                let duration = Double(key.durationMilliseconds) / 1_000
                var primary = PlaybackSegments.empty(duration: duration)
                if let url = Self.lookupURL(tmdbID: key.tmdbID, season: key.season,
                                            episode: key.episode,
                                            durationMilliseconds: key.durationMilliseconds),
                   let data = await Self.fetchJSON(from: url, using: transport),
                   let result = Self.validatedSegments(from: data, expectedSeason: key.season,
                                                       expectedEpisode: key.episode, duration: duration) {
                    primary = result
                }

                let primaryHasOpening = primary.segments.contains { $0.kind == .intro || $0.kind == .recap }
                let primaryHasCredits = primary.segments.contains { $0.kind == .credits }
                guard let imdbID = key.imdbID,
                      !primaryHasOpening || !primaryHasCredits,
                      let url = Self.alternativeLookupURL(imdbID: imdbID, season: key.season,
                                                          episode: key.episode),
                      let data = await Self.fetchJSON(from: url, using: transport),
                      let alternative = Self.validatedAlternativeSegments(
                        from: data, expectedIMDbID: imdbID, expectedSeason: key.season,
                        expectedEpisode: key.episode, duration: duration
                      ) else {
                    return .success(primary)
                }
                return .success(Self.merge(primary: primary, alternative: alternative))
            }
            flights[key] = task
        }

        let result = await task.value
        flights.removeValue(forKey: key)

        let value: PlaybackSegments
        let lifetime: TimeInterval
        switch result {
        case .success(let segments):
            value = segments
            lifetime = segments.segments.isEmpty ? 30 : 10 * 60
        case .failure:
            value = .empty(duration: duration)
            lifetime = 30
        }
        insert(value, for: key, expiresAt: Date().addingTimeInterval(lifetime))
        return value
    }

    private func insert(_ value: PlaybackSegments, for key: Key, expiresAt: Date) {
        accessOrder &+= 1
        cache[key] = CacheEntry(value: value, expiresAt: expiresAt, accessOrder: accessOrder)
        if cache.count > 32,
           let oldest = cache.min(by: { $0.value.accessOrder < $1.value.accessOrder })?.key {
            cache.removeValue(forKey: oldest)
        }
    }

    static func validatedSegments(
        from data: Data,
        expectedSeason: Int,
        expectedEpisode: Int,
        duration: Double
    ) -> PlaybackSegments? {
        guard duration.isFinite, duration > 0,
              let object = try? JSONSerialization.jsonObject(with: data),
              let response = object as? [String: Any],
              response["kind"] == nil || (response["kind"] as? String) == "media",
              response["versions"] == nil,
              response["episodes"] == nil,
              (response["type"] as? String)?.lowercased() == "tv",
              integer(response["season"]) == expectedSeason,
              integer(response["episode"]) == expectedEpisode,
              let groupDuration = integer(response["group_duration_ms"]), groupDuration > 0 else {
            return nil
        }

        let groupSeconds = Double(groupDuration) / 1_000
        let tolerance = max(2, min(8, duration * 0.005))
        guard abs(groupSeconds - duration) <= tolerance else { return nil }

        // These are conservative product thresholds, not a claim that the source's
        // confidence value is a calibrated probability of boundary correctness.
        let intros = parseSegments(response["intro"], kind: .intro, duration: duration)
        let credits = parseFinalCredits(response["credits"], duration: duration)
        return PlaybackSegments(duration: duration,
                               segments: (intros + credits).sorted {
                                   $0.start == $1.start ? $0.end < $1.end : $0.start < $1.start
                               },
                               source: (intros.isEmpty && credits.isEmpty) ? nil : "TheIntroDB")
    }

    static func validatedAlternativeSegments(
        from data: Data,
        expectedIMDbID: String,
        expectedSeason: Int,
        expectedEpisode: Int,
        duration: Double
    ) -> PlaybackSegments? {
        guard duration.isFinite, duration > 0,
              Self.validIMDbID(expectedIMDbID) == expectedIMDbID,
              let object = try? JSONSerialization.jsonObject(with: data),
              let response = object as? [String: Any],
              response["imdb_id"] as? String == expectedIMDbID,
              (response["media_type"] as? String)?.lowercased() == "tv",
              isExplicitFalseBoolean(response["is_movie"]),
              integer(response["season"]) == expectedSeason,
              integer(response["episode"]) == expectedEpisode,
              let outro = response["outro"] as? [String: Any],
              let outroStart = integer(outro["start_ms"]),
              let outroEnd = integer(outro["end_ms"]),
              acceptedAlternativeRecord(outro),
              outroStart >= 0, outroStart < outroEnd,
              Double(outroEnd) / 1_000 <= duration else {
            return nil
        }

        // IntroDB.app doesn't expose release groups. Its reviewed outro end is
        // the only per-record duration anchor available; retain all source times
        // verbatim and reject an outro that extends past the measured item.
        let tolerance = durationTolerance(duration)
        let outroStartSeconds = Double(outroStart) / 1_000
        let outroEndSeconds = Double(outroEnd) / 1_000
        guard abs(outroEndSeconds - duration) <= tolerance,
              outroStartSeconds >= duration * 0.75,
              duration - outroStartSeconds <= 15 * 60 else { return nil }

        let opening = parseAlternativeOpening(response["intro"], kind: .intro, duration: duration)
            ?? parseAlternativeOpening(response["recap"], kind: .recap, duration: duration)
        var segments = opening.map { [$0] } ?? []
        if response["post_credits"] == nil || response["post_credits"] is NSNull {
            segments.append(PlaybackSegment(kind: .credits,
                                            start: outroStartSeconds,
                                            end: outroEndSeconds))
        }
        return PlaybackSegments(duration: duration, segments: segments,
                               source: segments.isEmpty ? nil : "IntroDB.app")
    }

    private static func merge(primary: PlaybackSegments, alternative: PlaybackSegments) -> PlaybackSegments {
        var merged = primary.segments
        let primaryHasOpening = merged.contains { $0.kind == .intro || $0.kind == .recap }
        let primaryHasCredits = merged.contains { $0.kind == .credits }
        var alternativeContributed = false
        if !primaryHasOpening,
           let opening = alternative.segments.first(where: { $0.kind == .intro || $0.kind == .recap }) {
            merged.append(opening)
            alternativeContributed = true
        }
        if !primaryHasCredits,
           let credits = alternative.segments.first(where: { $0.kind == .credits }) {
            merged.append(credits)
            alternativeContributed = true
        }
        let sources = [primary.source, alternativeContributed ? alternative.source : nil].compactMap { $0 }
        let source = Array(Set(sources)).sorted().joined(separator: " + ")
        return PlaybackSegments(duration: primary.duration,
                                segments: merged.sorted {
                                    $0.start == $1.start ? $0.end < $1.end : $0.start < $1.start
                                },
                                source: source.isEmpty ? nil : source)
    }

    private static func parseAlternativeOpening(
        _ raw: Any?,
        kind: PlaybackSegmentKind,
        duration: Double
    ) -> PlaybackSegment? {
        guard let value = raw as? [String: Any],
              let startMilliseconds = integer(value["start_ms"]),
              let endMilliseconds = integer(value["end_ms"]),
              acceptedAlternativeRecord(value) else { return nil }
        let start = Double(startMilliseconds) / 1_000
        let end = Double(endMilliseconds) / 1_000
        let length = end - start
        guard start >= 0, start < end, end <= duration,
              (3...300).contains(length), start < min(1_200, duration / 2),
              end < duration - 30 else { return nil }
        return PlaybackSegment(kind: kind, start: start, end: end)
    }

    private static func acceptedAlternativeRecord(_ value: [String: Any]) -> Bool {
        // A conservative application heuristic over source evidence, not a
        // calibrated probability guarantee about the timestamp boundaries.
        guard let confidence = number(value["confidence"]), (0.95...1).contains(confidence),
              let count = integer(value["submission_count"]), count >= 1 else { return false }
        return true
    }

    private static func isExplicitFalseBoolean(_ raw: Any?) -> Bool {
        guard let value = raw as? NSNumber, CFGetTypeID(value) == CFBooleanGetTypeID() else { return false }
        return !value.boolValue
    }

    private static func durationTolerance(_ duration: Double) -> Double {
        max(2, min(8, duration * 0.005))
    }

    private static func validIMDbID(_ value: String?) -> String? {
        guard let value,
              value.range(of: #"^tt[0-9]{1,12}$"#, options: .regularExpression) != nil else { return nil }
        return value
    }

    private static func fetchJSON(from url: URL, using transport: Transport) async -> Data? {
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 5)
        request.httpMethod = "GET"
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        guard let response = try? await transport(request),
              response.statusCode == 200,
              response.url == url,
              response.mimeType?.lowercased().split(separator: ";", maxSplits: 1).first == "application/json",
              response.data.count <= 64 * 1024 else { return nil }
        return response.data
    }

    private static func parseSegments(
        _ raw: Any?,
        kind: PlaybackSegmentKind,
        duration: Double
    ) -> [PlaybackSegment] {
        guard let values = raw as? [[String: Any]] else { return [] }
        return values.prefix(16).compactMap { value in
            guard let confidence = number(value["confidence"]), (0.5...1).contains(confidence),
                  let count = integer(value["submission_count"]), count >= 2 else { return nil }

            let start: Double
            if value["start_ms"] is NSNull, kind == .intro {
                start = 0
            } else if let milliseconds = integer(value["start_ms"]) {
                start = Double(milliseconds) / 1_000
            } else {
                return nil
            }

            let end: Double
            if value["end_ms"] is NSNull, kind == .credits {
                end = duration
            } else if let milliseconds = integer(value["end_ms"]) {
                end = Double(milliseconds) / 1_000
            } else {
                return nil
            }

            guard start.isFinite, end.isFinite, start >= 0, start < end, end <= duration else { return nil }
            let length = end - start
            switch kind {
            case .intro, .recap:
                guard (3...300).contains(length), start < min(1_200, duration / 2), end < duration - 30 else { return nil }
            case .credits:
                guard start >= duration * 0.75, duration - start <= 15 * 60,
                      end >= duration - 2 else { return nil }
            }
            return PlaybackSegment(kind: kind, start: start, end: end)
        }
    }

    private static func parseFinalCredits(_ raw: Any?, duration: Double) -> [PlaybackSegment] {
        guard let values = raw as? [[String: Any]] else { return [] }
        let ordered = values.prefix(16).compactMap { value -> (start: Int, value: [String: Any])? in
            guard let start = integer(value["start_ms"]) else { return nil }
            return (start, value)
        }.sorted { $0.start < $1.start }
        guard let last = ordered.last?.value else { return [] }
        return parseSegments([last], kind: .credits, duration: duration)
    }

    private static func integer(_ raw: Any?) -> Int? {
        guard let value = number(raw), value.rounded(.towardZero) == value,
              value >= Double(Int.min), value < Double(Int.max) else { return nil }
        return Int(value)
    }

    private static func number(_ raw: Any?) -> Double? {
        guard let value = raw as? NSNumber,
              CFGetTypeID(value) != CFBooleanGetTypeID() else { return nil }
        let result = value.doubleValue
        return result.isFinite ? result : nil
    }

    private static func milliseconds(_ seconds: Double) -> Int64? {
        let value = seconds * 1_000
        guard value.isFinite, value > 0, value < Double(Int64.max) else { return nil }
        return Int64(value.rounded())
    }

    private static func lookupURL(tmdbID: Int, season: Int, episode: Int, durationMilliseconds: Int64) -> URL? {
        var components = URLComponents(string: "https://api.theintrodb.org/v4/media")
        components?.queryItems = [
            URLQueryItem(name: "tmdb_id", value: String(tmdbID)),
            URLQueryItem(name: "season", value: String(season)),
            URLQueryItem(name: "episode", value: String(episode)),
            URLQueryItem(name: "duration_ms", value: String(durationMilliseconds)),
            URLQueryItem(name: "merge_unknown", value: "false"),
            URLQueryItem(name: "details", value: "true"),
            URLQueryItem(name: "mapping", value: "official"),
        ]
        return components?.url
    }

    private static func alternativeLookupURL(imdbID: String, season: Int, episode: Int) -> URL? {
        var components = URLComponents(string: "https://api.introdb.app/segments")
        components?.queryItems = [
            URLQueryItem(name: "imdb_id", value: imdbID),
            URLQueryItem(name: "season", value: String(season)),
            URLQueryItem(name: "episode", value: String(episode)),
        ]
        return components?.url
    }

    private static func makeDefaultTransport() -> Transport {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 5
        configuration.timeoutIntervalForResource = 8
        configuration.urlCache = nil
        configuration.httpCookieStorage = nil
        configuration.httpCookieAcceptPolicy = .never
        configuration.httpShouldSetCookies = false
        configuration.waitsForConnectivity = false
        let session = URLSession(configuration: configuration, delegate: NoRedirectDelegate(), delegateQueue: nil)

        return { request in
            let (bytes, response) = try await session.bytes(for: request)
            guard let http = response as? HTTPURLResponse else {
                return PlaybackSegmentHTTPResponse(statusCode: 0, url: response.url,
                                                   mimeType: response.mimeType, data: Data())
            }
            var data = Data()
            data.reserveCapacity(8 * 1024)
            for try await byte in bytes {
                guard data.count < 64 * 1024 else { throw URLError(.dataLengthExceedsMaximum) }
                data.append(byte)
            }
            return PlaybackSegmentHTTPResponse(statusCode: http.statusCode, url: response.url,
                                               mimeType: response.mimeType, data: data)
        }
    }
}

private final class NoRedirectDelegate: NSObject, URLSessionTaskDelegate {
    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest,
        completionHandler: @escaping (URLRequest?) -> Void
    ) {
        completionHandler(nil)
    }
}
