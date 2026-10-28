import Foundation
import SwiftUI

enum MediaType: String, Codable, Hashable, Sendable {
    case movie
    case tv

    var displayName: String {
        switch self {
        case .movie: "Movie"
        case .tv: "TV Series"
        }
    }
}

struct MediaTitle: Codable, Hashable, Identifiable, Sendable {
    let tmdbID: Int
    let type: MediaType
    let title: String
    let overview: String
    let year: String
    let rating: Double
    let backdropURL: URL?
    let posterURL: URL?
    let imdbID: String?
    let malID: Int?
    let genres: [String]

    var id: String { "\(type.rawValue)-\(tmdbID)" }

    init(
        tmdbID: Int,
        type: MediaType,
        title: String,
        overview: String = "",
        year: String = "",
        rating: Double = 0.0,
        backdropURL: URL? = nil,
        posterURL: URL? = nil,
        imdbID: String? = nil,
        malID: Int? = nil,
        genres: [String] = []
    ) {
        self.tmdbID = tmdbID
        self.type = type
        self.title = title
        self.overview = overview
        self.year = year
        self.rating = rating
        self.backdropURL = backdropURL
        self.posterURL = posterURL
        self.imdbID = imdbID
        self.malID = malID
        self.genres = genres
    }

    enum CodingKeys: String, CodingKey {
        case tmdbID = "id"
        case type
        case title
        case overview
        case year
        case rating
        case backdropURL = "backdropUrl"
        case posterURL = "posterUrl"
        case imdbID
        case malID
        case genres
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        tmdbID = try container.decode(Int.self, forKey: .tmdbID)
        type = try container.decode(MediaType.self, forKey: .type)
        title = try container.decode(String.self, forKey: .title)
        overview = try container.decodeIfPresent(String.self, forKey: .overview) ?? ""
        year = try container.decodeIfPresent(String.self, forKey: .year) ?? ""
        rating = try container.decodeIfPresent(Double.self, forKey: .rating) ?? 0.0
        backdropURL = try container.decodeIfPresent(URL.self, forKey: .backdropURL)
        posterURL = try container.decodeIfPresent(URL.self, forKey: .posterURL)
        imdbID = try container.decodeIfPresent(String.self, forKey: .imdbID)
        malID = try container.decodeIfPresent(Int.self, forKey: .malID)
        genres = try container.decodeIfPresent([String].self, forKey: .genres) ?? []
    }
}

struct CatalogSection: Codable, Hashable, Identifiable, Sendable {
    let id: String
    let heading: String
    let titles: [MediaTitle]
}

struct CatalogResponse: Codable, Sendable {
    let configured: Bool
    let titles: [MediaTitle]
    let sections: [CatalogSection]?
    var featured: [MediaTitle]? = nil
}

struct CatalogGenerationStatus: Codable, Sendable {
    enum State: String, Codable, Sendable { case pending, ready, unavailable, failed }
    let status: State
    let updatedAt: String?
    let reason: String?
    enum CodingKeys: String, CodingKey {
        case status, reason
        case updatedAt = "updated_at"
    }
}

struct SearchResponse: Codable, Sendable {
    let titles: [MediaTitle]
    let page: Int
    let totalPages: Int
    let limit: Int
}

struct TitleDetails: Codable, Sendable {
    let runtime: Int?
    let genres: [String]
    let seasons: [SeasonSummary]
}

struct SeasonSummary: Codable, Hashable, Identifiable, Sendable {
    let number: Int
    let name: String
    let episodeCount: Int

    var id: Int { number }
}

struct Episode: Codable, Hashable, Identifiable, Sendable {
    let number: Int
    let name: String
    let overview: String
    let runtime: Int?
    let stillURL: URL?

    var id: Int { number }

    enum CodingKeys: String, CodingKey {
        case number
        case name
        case overview
        case runtime
        case stillURL = "stillUrl"
    }
}

// MARK: - Profiles & Preferences

enum ProfileColor {
    static let options: [String] = [
        "#446e88", // Blue
        "#916050", // Terracotta
        "#616b4b", // Olive
        "#76618e", // Violet
        "#93773c", // Gold
        "#536e70", // Teal
    ]

    static func name(for hex: String) -> String {
        switch hex.lowercased() {
        case "#446e88": "Blue"
        case "#916050": "Terracotta"
        case "#616b4b": "Olive"
        case "#76618e": "Violet"
        case "#93773c": "Gold"
        case "#536e70": "Teal"
        default: "Profile Color"
        }
    }

    static func color(for hex: String) -> Color {
        Color(hex: hex)
    }
}

struct LikedTitle: Codable, Hashable, Identifiable, Sendable {
    let id: Int
    let type: MediaType
    let title: String
}

struct ProfilePreferences: Codable, Hashable, Sendable {
    var genres: [String]
    var format: String // "both", "movie", "tv"
    var likedTitles: [LikedTitle]
    var completed: Bool

    static let empty = ProfilePreferences(
        genres: [],
        format: "both",
        likedTitles: [],
        completed: false
    )
}

struct Profile: Codable, Hashable, Identifiable, Sendable {
    let id: String
    var name: String
    var color: String
    var preferences: ProfilePreferences
}

struct GenreOption: Identifiable, Hashable, Sendable {
    let id: String
    let name: String

    static let all: [GenreOption] = [
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

struct GenreArtwork: Codable, Hashable, Sendable {
    let imageUrl: URL?
    let title: String
}

struct OnboardingTitlesResponse: Codable, Sendable {
    let titles: [MediaTitle]
}

// MARK: - Personal Library & Progress

struct WatchProgress: Codable, Hashable, Sendable {
    let title: MediaTitle
    let season: Int
    let episode: Int
    let episodeTitle: String
    let position: Double
    let duration: Double
    let watchedSeconds: Double
    let completed: Bool
    let updatedAt: Double
}

struct WatchProgressUpdate: Codable, Sendable {
    let title: MediaTitle
    let sessionId: String
    let sequence: Int
    let season: Int
    let episode: Int
    let episodeTitle: String
    let position: Double
    let duration: Double
    let watchedSeconds: Double
}

struct ProfileLibrary: Codable, Sendable {
    var list: [MediaTitle]
    var progress: [WatchProgress]

    static let empty = ProfileLibrary(list: [], progress: [])
}

struct SimpleStatusResponse: Codable, Sendable {
    let ok: Bool?
}

// MARK: - Color Hex Extension

extension Color {
    init(hex: String) {
        let cleanHex = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var int: UInt64 = 0
        Scanner(string: cleanHex).scanHexInt64(&int)
        let a, r, g, b: UInt64
        switch cleanHex.count {
        case 3: // RGB (12-bit)
            (a, r, g, b) = (255, (int >> 8) * 17, (int >> 4 & 0xF) * 17, (int & 0xF) * 17)
        case 6: // RGB (24-bit)
            (a, r, g, b) = (255, int >> 16, int >> 8 & 0xFF, int & 0xFF)
        case 8: // ARGB (32-bit)
            (a, r, g, b) = (int >> 24, int >> 16 & 0xFF, int >> 8 & 0xFF, int & 0xFF)
        default:
            (a, r, g, b) = (255, 100, 100, 100)
        }
        self.init(
            .sRGB,
            red: Double(r) / 255.0,
            green: Double(g) / 255.0,
            blue: Double(b) / 255.0,
            opacity: Double(a) / 255.0
        )
    }
}

extension ProfilePreferences {
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        genres = try container.decode([String].self, forKey: .genres)
        format = try container.decode(String.self, forKey: .format)
        completed = try container.decode(Bool.self, forKey: .completed)
        likedTitles = try container.decodeIfPresent([LikedTitle].self, forKey: .likedTitles) ?? []
    }
}

// Explicit choices are separate from measured playback progress.
enum RecommendationChoice: String, CaseIterable, Identifiable, Sendable {
    case hidden, watched, ignored, favorites
    var id: String { rawValue }
    var path: String { self == .favorites ? "favorites" : "recommendation-" + rawValue }
    var label: String {
        switch self {
        case .hidden: "Not for me"
        case .watched: "Already watched"
        case .ignored: "Don't use for my taste"
        case .favorites: "Like"
        }
    }
}

struct RecommendationChoices: Sendable {
    var hidden: [MediaTitle]
    var watched: [MediaTitle]
    var ignored: [MediaTitle]
    var favorites: [MediaTitle]
    var topics: [String]
    func titles(for choice: RecommendationChoice) -> [MediaTitle] {
        switch choice {
        case .hidden: hidden
        case .watched: watched
        case .ignored: ignored
        case .favorites: favorites
        }
    }
}

extension Notification.Name {
    static let streamTVRecommendationsChanged = Notification.Name("stream-tv.recommendations-changed")
}
