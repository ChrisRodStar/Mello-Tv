import Foundation

private actor SegmentFixture {
    var requests: [URLRequest] = []
    let status: Int
    let payload: Data
    let alternativePayload: Data?
    let mime: String
    let redirect: Bool

    init(status: Int = 200, payload: Data, alternativePayload: Data? = nil,
         mime: String = "application/json", redirect: Bool = false) {
        self.status = status; self.payload = payload; self.alternativePayload = alternativePayload
        self.mime = mime; self.redirect = redirect
    }
    func fetch(_ request: URLRequest) async throws -> PlaybackSegmentHTTPResponse {
        requests.append(request)
        try await Task.sleep(for: .milliseconds(50))
        let body = request.url?.host == "api.introdb.app" ? (alternativePayload ?? payload) : payload
        return PlaybackSegmentHTTPResponse(statusCode: status,
            url: redirect ? URL(string: "https://other.invalid/redirect") : request.url,
            mimeType: mime, data: body)
    }
}

private final class EpisodeResolverFixture: URLProtocol {
    static let lock = NSLock()
    static var scenario = "unordered"
    static func configure(_ value: String) {
        lock.lock(); defer { lock.unlock() }; scenario = value
    }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        Self.lock.lock(); let scenario = Self.scenario; Self.lock.unlock()
        let path = request.url!.path
        let result: [String: Any]
        if path.contains("/seasons/") {
            let season = Int(path.split(separator: "/").last!)!
            let numbers = scenario == "unordered" ? [4, 2, 1] : (season == 3 ? [2, 1] : [1, 3])
            result = ["data": ["show_id": 1396, "season": season, "episodes": numbers.map {
                ["episode": $0, "name": "Episode \($0)", "overview": "Fixture", "runtime": 45]
            }]]
        } else {
            let seasons: [[String: Any]] = scenario == "last" ? [["season":1,"name":"One","episode_count":2]] : [
                ["season":1,"name":"One","episode_count":2],
                ["season":2,"name":"Empty","episode_count":0],
                ["season":3,"name":"Three","episode_count":2],
            ]
            result = ["data": ["id":1396,"type":"tv","title":"Fixture","overview":"Fixture",
                               "rating":8,"details":true,"seasons":seasons]]
        }
        let bytes = try! JSONSerialization.data(withJSONObject: result)
        let response = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil,
            headerFields: ["Content-Type":"application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: bytes)
        client?.urlProtocolDidFinishLoading(self)
    }
}

@main
struct EpisodePromptChecks {
    static func require(_ condition: Bool, _ message: String = "Episode prompt check failed") {
        precondition(condition, message)
    }
    static let raw: [String: Any] = [
        "tvdb_id": 81189, "type": "tv", "season": 1, "episode": 1,
        "group_duration_ms": 3_500_000,
        "intro": [["start_ms": 228566, "end_ms": 246538, "confidence": 0.5662989313773523, "submission_count": 6]],
        "credits": [["start_ms": 3431000, "end_ms": NSNull(), "confidence": 0.25, "submission_count": 2]],
    ]
    static let supernaturalE15: [String: Any] = [
        "imdb_id": "tt0460681", "media_type": "tv", "is_movie": false,
        "season": 1, "episode": 15,
        "intro": NSNull(),
        "recap": ["start_sec": 0, "end_sec": 32, "start_ms": 0, "end_ms": 32_000,
                   "confidence": 1, "submission_count": 1],
        "outro": ["start_sec": 2552, "end_sec": 2599, "start_ms": 2_552_000,
                   "end_ms": 2_599_000, "confidence": 1, "submission_count": 1],
        "post_credits": NSNull(), "no_intro": false, "no_recap": false, "no_outro": false,
    ]

    static func data(_ value: [String: Any]) throws -> Data {
        try JSONSerialization.data(withJSONObject: value)
    }
    static func validate(_ value: [String: Any], duration: Double = 3500) throws -> PlaybackSegments? {
        PlaybackSegmentService.validatedSegments(from: try data(value),
            expectedSeason: 1, expectedEpisode: 1, duration: duration)
    }
    static func validateAlternative(_ value: [String: Any], duration: Double = 2600.64) throws -> PlaybackSegments? {
        PlaybackSegmentService.validatedAlternativeSegments(from: try data(value),
            expectedIMDbID: "tt0460681", expectedSeason: 1, expectedEpisode: 15, duration: duration)
    }

    @MainActor
    static func main() async throws {
        let valid = try validate(raw)!
        require(valid.segments.count == 1 && valid.segments[0].kind == .intro)
        require(abs(valid.segments[0].start - 228.566) < 0.001)
        require(abs(valid.segments[0].end - 246.538) < 0.001)
        print("PASS real raw API fixture without wrapper kind accepts supported intro and rejects low-support credits")

        for change in [
            ["kind": "versions"], ["versions": []], ["episodes": []],
            ["type": "movie"], ["season": 2], ["episode": 2],
            ["group_duration_ms": 3_600_000], ["group_duration_ms": true],
            ["group_duration_ms": 3_500_000.5],
        ] as [[String: Any]] {
            var candidate = raw
            candidate.merge(change) { _, replacement in replacement }
            require(try validate(candidate) == nil, "wrong identity/schema/edition must not produce skip actions")
        }
        require(try validate(raw, duration: .nan) == nil)
        require(try validate(raw, duration: 3510) == nil)
        require(try validate(raw, duration: 3502)?.segments.count == 1)
        print("PASS identity, raw schema, finite duration, and release-duration guards")

        let invalidIntros: [[String: Any]] = [
            ["start_ms": -1, "end_ms": 30000, "confidence": 0.9, "submission_count": 4],
            ["start_ms": 30000, "end_ms": 20000, "confidence": 0.9, "submission_count": 4],
            ["start_ms": 0, "end_ms": 900000, "confidence": 0.9, "submission_count": 4],
            ["start_ms": 0, "end_ms": 30000, "confidence": 0.49, "submission_count": 9],
            ["start_ms": 0, "end_ms": 30000, "confidence": 0.9, "submission_count": 1],
            ["start_ms": 0, "end_ms": 30000, "confidence": true, "submission_count": 4],
            ["start_ms": 2000000, "end_ms": 2030000, "confidence": 0.9, "submission_count": 4],
        ]
        for intro in invalidIntros {
            var candidate = raw; candidate["intro"] = [intro]
            require(try validate(candidate)?.segments.isEmpty == true)
        }
        var nullStart = raw
        nullStart["intro"] = [["start_ms": NSNull(), "end_ms": 31000, "confidence": 0.7, "submission_count": 3]]
        require(try validate(nullStart)?.segments.first?.start == 0)
        print("PASS malformed, low-confidence, low-support, and implausible intro ranges are omitted")

        var creditRanges = raw
        creditRanges["credits"] = [
            ["start_ms": 3200000, "end_ms": 3250000, "confidence": 0.9, "submission_count": 3],
            ["start_ms": 3450000, "end_ms": NSNull(), "confidence": 0.9, "submission_count": 3],
        ]
        let credits = try validate(creditRanges)!.segments.filter { $0.kind == .credits }
        require(credits.count == 1 && credits[0].start == 3450 && credits[0].end == 3500)
        print("PASS only the final duration-matched credit interval is retained")

        let supernatural = try validateAlternative(supernaturalE15)!
        require(supernatural.segments.count == 2)
        require(supernatural.segments[0].kind == .recap && supernatural.segments[0].openingSkipLabel == "Skip Recap")
        require(supernatural.segments[0].start == 0 && supernatural.segments[0].end == 32)
        require(supernatural.segments[1].kind == .credits && supernatural.segments[1].openingSkipLabel == nil)
        require(supernatural.segments[1].start == 2552 && supernatural.segments[1].end == 2599)
        require(supernatural.source == "IntroDB.app")
        print("PASS real Supernatural S1E15 fallback fixture keeps exact recap/outro boundaries")

        for change in [
            ["imdb_id": "tt0000000"], ["media_type": "movie"], ["is_movie": 0],
            ["season": 2], ["episode": 16], ["outro": NSNull()],
            ["outro": ["start_ms": 2_552_000, "end_ms": 2_500_000,
                       "confidence": 1, "submission_count": 1]],
            ["outro": ["start_ms": 1_000_000, "end_ms": 2_599_000,
                       "confidence": 1, "submission_count": 1]],
            ["outro": ["start_ms": 2_552_000, "end_ms": 2_599_000,
                       "confidence": 0.94, "submission_count": 1]],
            ["outro": ["start_ms": 2_552_000, "end_ms": 2_599_000,
                       "confidence": 1, "submission_count": 0]],
        ] as [[String: Any]] {
            var candidate = supernaturalE15
            candidate.merge(change) { _, replacement in replacement }
            require(try validateAlternative(candidate) == nil,
                    "fallback must reject identity, media type, duration anchor, and low-evidence failures")
        }
        require(try validateAlternative(supernaturalE15, duration: 2500) == nil,
                "fallback rejects an outro anchor that cannot belong to the measured cut")
        require(PlaybackSegmentService.validatedAlternativeSegments(from: Data("bad json".utf8),
            expectedIMDbID: "tt0460681", expectedSeason: 1, expectedEpisode: 15,
            duration: 2600.64) == nil)
        var noMatchingDurationAnchor = supernaturalE15
        noMatchingDurationAnchor["outro"] = ["start_ms": 2_552_000, "end_ms": 2_590_000,
                                               "confidence": 1, "submission_count": 1]
        require(try validateAlternative(noMatchingDurationAnchor) == nil,
                "fallback rejects a finite outro with mismatched item duration")
        var postCredits = supernaturalE15
        postCredits["post_credits"] = ["start_ms": 2_599_500, "end_ms": 2_600_640,
                                       "confidence": 1, "submission_count": 1]
        let afterCredits = try validateAlternative(postCredits)!
        require(afterCredits.segments.map(\.kind) == [.recap], "outro is not end credits when post-credit content exists")
        var noUsableOpening = supernaturalE15
        noUsableOpening["recap"] = ["start_ms": 0, "end_ms": 600_000, "confidence": 1, "submission_count": 1]
        require(try validateAlternative(noUsableOpening)?.segments.map(\.kind) == [.credits])
        print("PASS fallback rejects mismatches, unanchored/weak data, and post-credit endings")

        let title = MediaTitle(tmdbID: 1396, type: .tv, title: "Synthetic episode")
        let request = PlaybackRequest(titleID: 1396, type: .tv, title: title.title,
            profileID: "never-sent-to-timing-service", season: 1, episode: 1, catalogTitle: title)
        let fixture = SegmentFixture(payload: try data(raw))
        let service = PlaybackSegmentService(transport: { try await fixture.fetch($0) })
        async let first = service.segments(for: request, duration: 3500)
        async let second = service.segments(for: request, duration: 3500)
        let coalesced = await (first, second)
        require(coalesced.0.segments.count == 1 && coalesced.1.segments.count == 1)
        _ = await service.segments(for: request, duration: 3500)
        let sent = await fixture.requests
        require(sent.count == 1, "concurrent and cached lookups share one network request")
        let url = sent[0].url!
        require(url.scheme == "https" && url.host == "api.theintrodb.org" && url.path == "/v4/media")
        let query = Dictionary(uniqueKeysWithValues: URLComponents(url: url, resolvingAgainstBaseURL: false)!.queryItems!.map { ($0.name, $0.value ?? "") })
        require(query["tmdb_id"] == "1396" && query["season"] == "1" && query["episode"] == "1")
        require(query["duration_ms"] == "3500000" && query["details"] == "true" && query["merge_unknown"] == "false")
        require(!url.absoluteString.contains(request.profileID))
        require(sent[0].value(forHTTPHeaderField: "Authorization") == nil && sent[0].value(forHTTPHeaderField: "Cookie") == nil)
        print("PASS bounded client lookup coalescing/cache and episode-only request contract")

        let supernaturalTitle = MediaTitle(tmdbID: 1622, type: .tv, title: "Supernatural", imdbID: "tt0460681")
        let supernaturalRequest = PlaybackRequest(titleID: 1622, type: .tv, title: "Supernatural",
            profileID: "fixture-profile", season: 1, episode: 15, catalogTitle: supernaturalTitle)
        let rawPrimary = try data(raw)
        let rawAlternative = try data(supernaturalE15)
        let fallbackFixture = SegmentFixture(payload: rawPrimary, alternativePayload: rawAlternative)
        let fallbackService = PlaybackSegmentService(transport: { try await fallbackFixture.fetch($0) })
        let fallbackResult = await fallbackService.segments(for: supernaturalRequest, duration: 2600.64)
        require(fallbackResult.segments.map(\.kind) == [.recap, .credits])
        let fallbackRequests = await fallbackFixture.requests
        require(fallbackRequests.count == 2)
        let alternativeURL = fallbackRequests[1].url!
        require(alternativeURL.host == "api.introdb.app" && alternativeURL.path == "/segments")
        let alternativeQuery = Dictionary(uniqueKeysWithValues: URLComponents(url: alternativeURL,
            resolvingAgainstBaseURL: false)!.queryItems!.map { ($0.name, $0.value ?? "") })
        require(alternativeQuery["imdb_id"] == "tt0460681" && alternativeQuery["season"] == "1" && alternativeQuery["episode"] == "15")
        require(!alternativeURL.absoluteString.contains("fixture-profile"))
        let noIMDbTitle = MediaTitle(tmdbID: 1622, type: .tv, title: "Supernatural")
        let noIMDbRequest = PlaybackRequest(titleID: 1622, type: .tv, title: "Supernatural",
            profileID: "fixture-profile", season: 1, episode: 15, catalogTitle: noIMDbTitle)
        require(await fallbackService.segments(for: noIMDbRequest, duration: 2600.64).segments.isEmpty,
                "a cached IMDb-enriched result must not poison a request without IMDb")
        require(await fallbackFixture.requests.count == 3,
                "cache keys distinguish otherwise identical requests with and without an IMDb ID")
        print("PASS direct IMDb fallback is bounded, identity-scoped, and not reused without IMDb")

        var partialPrimary = raw
        partialPrimary["episode"] = 15
        partialPrimary["group_duration_ms"] = 2_600_640
        partialPrimary["intro"] = [["start_ms": 40_000, "end_ms": 70_000,
                                     "confidence": 0.9, "submission_count": 4]]
        partialPrimary["credits"] = []
        let partialFixture = SegmentFixture(payload: try data(partialPrimary), alternativePayload: rawAlternative)
        let partialService = PlaybackSegmentService(transport: { try await partialFixture.fetch($0) })
        let partial = await partialService.segments(for: supernaturalRequest, duration: 2600.64)
        require(partial.segments.map(\.kind) == [.intro, .credits],
                "fallback fills missing credits without replacing valid primary intro")
        require(partial.segments.first?.start == 40 && partial.segments.first?.end == 70)
        require(partial.source == "IntroDB.app + TheIntroDB")
        print("PASS fallback fills a missing segment kind while preserving primary source markers")

        for fixture in [
            SegmentFixture(status: 429, payload: try data(raw)),
            SegmentFixture(payload: try data(raw), mime: "text/html"),
            SegmentFixture(payload: try data(raw), redirect: true),
            SegmentFixture(payload: Data(repeating: 32, count: 65537)),
        ] {
            let service = PlaybackSegmentService(transport: { try await fixture.fetch($0) })
            let failure = await service.segments(for: request, duration: 3500)
            require(failure.segments.isEmpty)
            _ = await service.segments(for: request, duration: 3500)
            require(await fixture.requests.count == 1, "failures are briefly cached rather than retried on each frame")
        }
        print("PASS throttling, redirects, invalid content, and oversized responses fail without skip actions")

        var prompt = EpisodePromptState()
        prompt.bind(to: "episode-one")
        prompt.consider(position: 970, cardTrigger: 970, countdownTrigger: 990, naturallyAdvanced: true)
        require(prompt.isVisible && !prompt.isCountingDown && !prompt.tick())
        prompt.consider(position: 992, cardTrigger: 970, countdownTrigger: 990, naturallyAdvanced: true)
        require(prompt.isCountingDown && prompt.secondsRemaining == 10)
        for _ in 0..<9 { require(!prompt.tick()) }
        require(prompt.tick() && prompt.secondsRemaining == 0)
        print("PASS near-end card precedes a visible ten-second countdown")

        prompt.keepWatching()
        prompt.consider(position: 999, cardTrigger: 970, countdownTrigger: 990, naturallyAdvanced: true)
        require(!prompt.isVisible && !prompt.tick())
        prompt.bind(to: "episode-two")
        require(!prompt.isCancelled && !prompt.isVisible && prompt.secondsRemaining == 10)
        prompt.seekOrScrub(to: 995)
        prompt.consider(position: 995, cardTrigger: 970, countdownTrigger: 990, naturallyAdvanced: false)
        require(!prompt.isVisible && !prompt.isCountingDown)
        prompt.consider(position: 997, cardTrigger: 970, countdownTrigger: 990, naturallyAdvanced: true)
        require(prompt.isVisible && prompt.isCountingDown)
        prompt.backgrounded(at: 997)
        prompt.consider(position: 997, cardTrigger: 970, countdownTrigger: 990, naturallyAdvanced: false)
        require(!prompt.isVisible && !prompt.tick())
        prompt.naturalEnd()
        require(!prompt.isCountingDown)
        print("PASS Keep watching, episode replacement, seeks, and backgrounding prevent stale autoplay")

        var retries = NextEpisodeLookupState()
        require(retries.beginAttempt(now: 100))
        retries.fail(now: 100) // includes cancellation while backgrounding
        require(retries.attempts == 1 && retries.retryAt == 105)
        require(!retries.beginAttempt(now: 104.9))
        require(retries.beginAttempt(now: 105))
        retries.fail(now: 105)
        require(retries.attempts == 2 && retries.retryAt == 120)
        require(!retries.beginAttempt(now: 119.9))
        require(retries.beginAttempt(now: 120))
        retries.fail(now: 120)
        require(retries.attempts == 3 && retries.retryAt == nil)
        require(!retries.beginAttempt(now: 10_000), "failed metadata retries are bounded per item")

        var foundSuccessor = NextEpisodeLookupState()
        require(foundSuccessor.beginAttempt(now: 1))
        foundSuccessor.complete(foundSuccessor: true)
        require(foundSuccessor.hasSuccessor && !foundSuccessor.beginAttempt(now: 10_000))
        var knownLastEpisode = NextEpisodeLookupState()
        require(knownLastEpisode.beginAttempt(now: 1))
        knownLastEpisode.complete(foundSuccessor: false)
        require(knownLastEpisode.hasNoSuccessor && !knownLastEpisode.beginAttempt(now: 10_000))
        print("PASS next-episode lookup retries use 5s/15s backoff, cap at three, and cache found/no-successor results")

        let resolverConfig = URLSessionConfiguration.ephemeral
        resolverConfig.protocolClasses = [EpisodeResolverFixture.self]
        let resolverAPI = MelloTVAPI(baseURL: URL(string: "https://fixture.invalid")!, session: URLSession(configuration: resolverConfig))
        EpisodeResolverFixture.configure("unordered")
        let successor = try await NextEpisodeResolver.next(after: request, api: resolverAPI)
        require(successor?.season == 1 && successor?.episode == 2, "use sorted successor, not payload order")
        let lastInSeason = PlaybackRequest(titleID: 1396, type: .tv, title: title.title,
            profileID: "fixture", season: 1, episode: 3, catalogTitle: title)
        EpisodeResolverFixture.configure("boundary")
        let rollover = try await NextEpisodeResolver.next(after: lastInSeason, api: resolverAPI)
        require(rollover?.season == 3 && rollover?.episode == 1, "skip empty seasons and choose first available episode")
        EpisodeResolverFixture.configure("last")
        require(try await NextEpisodeResolver.next(after: lastInSeason, api: resolverAPI) == nil,
                "last available episode has no autoplay successor")
        print("PASS episode ordering, empty-season rollover, and final-episode resolution")

        if CommandLine.arguments.contains("--live") {
            let live = await PlaybackSegmentService.shared.segments(for: request, duration: 3500)
            print("LIVE duration-matched Breaking Bad S1E1 markers:", live.segments.map {
                "\($0.kind.rawValue) \($0.start)-\($0.end)s"
            }, "source:", live.source ?? "none")
        }
        if CommandLine.arguments.contains("--live-supernatural") {
            let live = await PlaybackSegmentService.shared.segments(for: supernaturalRequest, duration: 2600.64)
            require(live.segments.contains { $0.kind == .recap && $0.start == 0 && $0.end == 32 },
                    "live Supernatural S1E15 should expose its reviewed recap")
            require(live.segments.contains { $0.kind == .credits && $0.start == 2552 && $0.end == 2599 },
                    "live Supernatural S1E15 should expose its duration-anchored outro")
            print("LIVE Supernatural S1E15 markers:", live.segments.map {
                "\($0.kind.rawValue) \($0.start)-\($0.end)s"
            }, "source:", live.source ?? "none")
        }
        print("All episode prompt checks passed")
    }
}
