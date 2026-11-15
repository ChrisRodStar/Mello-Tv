import Foundation
import MelloKit

/// Category filter for 10-foot tvOS catalog tabs.
public enum CatalogCategory: String, CaseIterable, Identifiable, Hashable, Sendable {
    case home
    case movies
    case tv

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .home: "Home"
        case .movies: "Movies"
        case .tv: "TV"
        }
    }

    public var mediaTypeFilter: MediaType? {
        switch self {
        case .home: nil
        case .movies: .movie
        case .tv: .tv
        }
    }
}
