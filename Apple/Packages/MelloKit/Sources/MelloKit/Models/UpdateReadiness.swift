import Foundation

public struct UpdateReadiness: Codable, Sendable, Equatable {
    public let safeToRestart: Bool

    public init(safeToRestart: Bool) {
        self.safeToRestart = safeToRestart
    }

    enum CodingKeys: String, CodingKey {
        case safeToRestart = "safe_to_restart"
    }
}
