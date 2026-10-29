import Foundation

public struct DiscoveredServer: Identifiable, Hashable, Sendable {
    public let id: String
    public let name: String
    public let primaryURL: URL
    public let endpoints: [URL]

    public init(id: String, name: String, primaryURL: URL, endpoints: [URL]) {
        self.id = id
        self.name = name
        self.primaryURL = primaryURL
        self.endpoints = endpoints
    }
}

