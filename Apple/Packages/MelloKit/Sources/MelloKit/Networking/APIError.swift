import Foundation

public enum APIError: LocalizedError, Sendable, Equatable {
    case invalidAddress
    case invalidResponse
    case invalidData
    case serverStatus(Int)
    case serverMessage(String)
    case connection(String)
    case service(status: Int, code: String, message: String)

    public var isRetryablePlayback: Bool {
        switch self {
        case .connection:
            return true
        case .service(let status, let code, _):
            return code == "playback_busy" || status == 502 || status == 504
        default:
            return false
        }
    }

    public var isExpiredPlayback: Bool {
        if case .service(_, let code, _) = self {
            return code == "session_expired" || code == "source_expired"
        }
        return false
    }

    public var requiresSourceRefresh: Bool {
        if case .service(_, let code, _) = self {
            return code == "source_refresh_required"
        }
        return false
    }

    public var isTerminalMutation: Bool {
        if case .service(_, let code, _) = self {
            return ["invalid_input", "invalid_selection", "unsupported_format", "renew_too_early"].contains(code)
        }
        return false
    }

    public var isDefinitiveQualityUpgradeFailure: Bool {
        guard case .service(let status, let code, _) = self else { return false }
        return (status == 422 && ["no_source", "quality_unavailable"].contains(code))
            || (status == 409 && code == "active_quality_unknown")
    }

    public var errorDescription: String? {
        switch self {
        case .invalidAddress:
            return "The server address is invalid."
        case .invalidResponse:
            return "The server returned an unexpected response."
        case .invalidData:
            return "The server response couldn’t be read."
        case .serverStatus(404):
            return "This item isn’t cached yet. Try again shortly."
        case .serverStatus(let code):
            return "The server returned an error (\(code))."
        case .serverMessage(let message), .connection(let message), .service(_, _, let message):
            return message
        }
    }
}
