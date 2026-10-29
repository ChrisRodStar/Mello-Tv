import Foundation

public enum MediaType: String, Codable, Hashable, Sendable {
    case movie
    case tv

    public var displayName: String {
        switch self {
        case .movie: "Movie"
        case .tv: "TV Series"
        }
    }
}

public struct MediaTitle: Codable, Hashable, Identifiable, Sendable {
    public let tmdbID: Int
    public let type: MediaType
    public let title: String
    public let overview: String
    public let year: String
    public let rating: Double
    public let backdropURL: URL?
    public let posterURL: URL?
    public let imdbID: String?
    public let malID: Int?
    public let genres: [String]

    public var id: String { "\(type.rawValue)-\(tmdbID)" }

    public init(
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

    public enum CodingKeys: String, CodingKey {
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

    public init(from decoder: Decoder) throws {
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

public struct CatalogSection: Codable, Hashable, Identifiable, Sendable {
    public let id: String
    public let heading: String
    public let titles: [MediaTitle]

    public init(id: String, heading: String, titles: [MediaTitle]) {
        self.id = id
        self.heading = heading
        self.titles = titles
    }
}

public struct CatalogResponse: Codable, Sendable {
    public let configured: Bool
    public let titles: [MediaTitle]
    public let sections: [CatalogSection]?
    public var featured: [MediaTitle]?

    public init(configured: Bool, titles: [MediaTitle], sections: [CatalogSection]? = nil, featured: [MediaTitle]? = nil) {
        self.configured = configured
        self.titles = titles
        self.sections = sections
        self.featured = featured
    }
}

public struct CatalogGenerationStatus: Codable, Sendable {
    public enum State: String, Codable, Sendable { case pending, ready, unavailable, failed }
    public let status: State
    public let updatedAt: String?
    public let reason: String?

    public enum CodingKeys: String, CodingKey {
        case status, reason
        case updatedAt = "updated_at"
    }

    public init(status: State, updatedAt: String? = nil, reason: String? = nil) {
        self.status = status
        self.updatedAt = updatedAt
        self.reason = reason
    }
}

public struct SearchResponse: Codable, Sendable {
    public let titles: [MediaTitle]
    public let page: Int
    public let totalPages: Int
    public let limit: Int

    public init(titles: [MediaTitle], page: Int, totalPages: Int, limit: Int) {
        self.titles = titles
        self.page = page
        self.totalPages = totalPages
        self.limit = limit
    }
}

public struct TitleDetails: Codable, Sendable {
    public let runtime: Int?
    public let genres: [String]
    public let seasons: [SeasonSummary]

    public init(runtime: Int? = nil, genres: [String] = [], seasons: [SeasonSummary] = []) {
        self.runtime = runtime
        self.genres = genres
        self.seasons = seasons
    }
}

public struct SeasonSummary: Codable, Hashable, Identifiable, Sendable {
    public let number: Int
    public let name: String
    public let episodeCount: Int

    public var id: Int { number }

    public init(number: Int, name: String, episodeCount: Int) {
        self.number = number
        self.name = name
        self.episodeCount = episodeCount
    }
}

public struct Episode: Codable, Hashable, Identifiable, Sendable {
    public let number: Int
    public let name: String
    public let overview: String
    public let runtime: Int?
    public let stillURL: URL?

    public var id: Int { number }

    public enum CodingKeys: String, CodingKey {
        case number
        case name
        case overview
        case runtime
        case stillURL = "stillUrl"
    }

    public init(number: Int, name: String, overview: String = "", runtime: Int? = nil, stillURL: URL? = nil) {
        self.number = number
        self.name = name
        self.overview = overview
        self.runtime = runtime
        self.stillURL = stillURL
    }
}

