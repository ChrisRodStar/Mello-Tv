import Foundation
import SwiftUI

public enum ProfileColor {
    public static let options: [String] = [
        "#446e88", // Blue
        "#916050", // Terracotta
        "#616b4b", // Olive
        "#76618e", // Violet
        "#93773c", // Gold
        "#536e70", // Teal
    ]

    public static func name(for hex: String) -> String {
        switch hex.lowercased() {
        case "#446e88": return "Blue"
        case "#916050": return "Terracotta"
        case "#616b4b": return "Olive"
        case "#76618e": return "Violet"
        case "#93773c": return "Gold"
        case "#536e70": return "Teal"
        default: return "Profile Color"
        }
    }

    public static func color(for hex: String) -> Color {
        Color(hex: hex)
    }
}

public struct LikedTitle: Codable, Hashable, Identifiable, Sendable {
    public let id: Int
    public let type: MediaType
    public let title: String

    public init(id: Int, type: MediaType, title: String) {
        self.id = id
        self.type = type
        self.title = title
    }
}

public struct ProfilePreferences: Codable, Hashable, Sendable {
    public var genres: [String]
    public var format: String // "both", "movie", "tv"
    public var likedTitles: [LikedTitle]
    public var completed: Bool

    public static let empty = ProfilePreferences(
        genres: [],
        format: "both",
        likedTitles: [],
        completed: false
    )

    public init(genres: [String], format: String, likedTitles: [LikedTitle], completed: Bool) {
        self.genres = genres
        self.format = format
        self.likedTitles = likedTitles
        self.completed = completed
    }

    public enum CodingKeys: String, CodingKey {
        case genres, format, likedTitles, completed
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        genres = try container.decode([String].self, forKey: .genres)
        format = try container.decode(String.self, forKey: .format)
        completed = try container.decode(Bool.self, forKey: .completed)
        likedTitles = try container.decodeIfPresent([LikedTitle].self, forKey: .likedTitles) ?? []
    }
}

public struct Profile: Codable, Hashable, Identifiable, Sendable {
    public let id: String
    public var name: String
    public var color: String
    public var preferences: ProfilePreferences

    public init(id: String, name: String, color: String, preferences: ProfilePreferences) {
        self.id = id
        self.name = name
        self.color = color
        self.preferences = preferences
    }
}

public struct GenreOption: Identifiable, Hashable, Sendable {
    public let id: String
    public let name: String

    public init(id: String, name: String) {
        self.id = id
        self.name = name
    }

    public static let all: [GenreOption] = [
        GenreOption(id: "action", name: "Action & adventure"),
        GenreOption(id: "comedy", name: "Comedy"),
        GenreOption(id: "drama", name: "Drama"),
        GenreOption(id: "crime", name: "Crime & mystery"),
        GenreOption(id: "scifi", name: "Sci-fi & fantasy"),
        GenreOption(id: "animation", name: "Anime"),
        GenreOption(id: "documentary", name: "Documentaries"),
        GenreOption(id: "horror", name: "Horror"),
    ]
}

public struct GenreArtwork: Codable, Hashable, Sendable {
    public let imageUrl: URL?
    public let title: String

    public init(imageUrl: URL?, title: String) {
        self.imageUrl = imageUrl
        self.title = title
    }
}

public struct OnboardingTitlesResponse: Codable, Sendable {
    public let titles: [MediaTitle]

    public init(titles: [MediaTitle]) {
        self.titles = titles
    }
}

public enum RecommendationChoice: String, CaseIterable, Identifiable, Sendable {
    case hidden, watched, ignored, favorites
    public var id: String { rawValue }
    public var path: String { self == .favorites ? "favorites" : "recommendation-" + rawValue }
    public var label: String {
        switch self {
        case .hidden: "Not for me"
        case .watched: "Already watched"
        case .ignored: "Don't use for my taste"
        case .favorites: "Like"
        }
    }
}

public struct RecommendationChoices: Sendable {
    public var hidden: [MediaTitle]
    public var watched: [MediaTitle]
    public var ignored: [MediaTitle]
    public var favorites: [MediaTitle]
    public var topics: [String]

    public init(
        hidden: [MediaTitle] = [],
        watched: [MediaTitle] = [],
        ignored: [MediaTitle] = [],
        favorites: [MediaTitle] = [],
        topics: [String] = []
    ) {
        self.hidden = hidden
        self.watched = watched
        self.ignored = ignored
        self.favorites = favorites
        self.topics = topics
    }

    public func titles(for choice: RecommendationChoice) -> [MediaTitle] {
        switch choice {
        case .hidden: hidden
        case .watched: watched
        case .ignored: ignored
        case .favorites: favorites
        }
    }
}

public extension Notification.Name {
    static let melloTVRecommendationsChanged = Notification.Name("stream-tv.recommendations-changed")
    static let streamTVRecommendationsChanged = melloTVRecommendationsChanged
}

