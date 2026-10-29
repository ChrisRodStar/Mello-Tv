import Foundation

public struct ServerSnapshot: Codable, Sendable {
    public let appVersion: String
    public let catalogSnapshot: String
    public let catalogStatus: String
    public let uptimeSeconds: Int
    public let endpoints: [String]
    public let media: MediaStatus
    public let providerUpdates: ProviderUpdates

    public init(
        appVersion: String,
        catalogSnapshot: String,
        catalogStatus: String,
        uptimeSeconds: Int,
        endpoints: [String],
        media: MediaStatus,
        providerUpdates: ProviderUpdates
    ) {
        self.appVersion = appVersion
        self.catalogSnapshot = catalogSnapshot
        self.catalogStatus = catalogStatus
        self.uptimeSeconds = uptimeSeconds
        self.endpoints = endpoints
        self.media = media
        self.providerUpdates = providerUpdates
    }

    public struct MediaStatus: Codable, Sendable {
        public let configured: Bool
        public let providers: [ProviderStatus]

        public init(configured: Bool, providers: [ProviderStatus]) {
            self.configured = configured
            self.providers = providers
        }
    }

    public struct ProviderUpdates: Codable, Sendable {
        public let mode: String
        public let available: Bool?

        public init(mode: String, available: Bool?) {
            self.mode = mode
            self.available = available
        }
    }

    public struct ProviderStatus: Codable, Identifiable, Sendable {
        public let id: String
        public let version: String
        public let lastObservedAt: Double?
        public let lastFailure: String?
        public let candidateCount: Int?

        public init(
            id: String,
            version: String,
            lastObservedAt: Double?,
            lastFailure: String?,
            candidateCount: Int?
        ) {
            self.id = id
            self.version = version
            self.lastObservedAt = lastObservedAt
            self.lastFailure = lastFailure
            self.candidateCount = candidateCount
        }

        public var name: String {
            ["vidcore": "VidCore", "vidlink": "VidLink", "vidsrc": "VidSrc",
             "vidfast": "VidFast", "videasy": "VidEasy"][id] ?? id.capitalized
        }

        public var observedAt: Date? {
            lastObservedAt.map(Date.init(timeIntervalSince1970:))
        }

        public var needsAttention: Bool {
            guard lastFailure != nil, let date = observedAt else { return false }
            return Date().timeIntervalSince(date) < 3600
        }

        public var activity: String {
            guard observedAt != nil else { return "Waiting for activity" }
            switch lastFailure {
            case "timeout": return "Last request timed out"
            case "protocol_change": return "Response changed"
            case "upstream_failure": return "Upstream request failed"
            case .some: return "Last request failed"
            case .none: return (candidateCount ?? 0) > 0 ? "Sources returned" : "No matches on last request"
            }
        }
    }
}

