import Foundation

struct ServerSnapshot: Decodable {
    let appVersion: String
    let catalogSnapshot: String
    let catalogStatus: String
    let uptimeSeconds: Int
    let endpoints: [String]
    let media: MediaStatus
    let providerUpdates: ProviderUpdates

    struct MediaStatus: Decodable {
        let configured: Bool
        let providers: [ProviderStatus]
    }

    struct ProviderUpdates: Decodable {
        let mode: String
        let available: Bool?
    }

    struct ProviderStatus: Decodable, Identifiable {
        let id: String
        let version: String
        let lastObservedAt: Double?
        let lastFailure: String?
        let candidateCount: Int?

        var name: String {
            ["vidcore": "VidCore", "vidlink": "VidLink", "vidsrc": "VidSrc",
             "vidfast": "VidFast", "videasy": "VidEasy"][id] ?? id.capitalized
        }

        var observedAt: Date? { lastObservedAt.map(Date.init(timeIntervalSince1970:)) }
        var needsAttention: Bool {
            guard lastFailure != nil, let date = observedAt else { return false }
            return Date().timeIntervalSince(date) < 3600
        }

        var activity: String {
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

