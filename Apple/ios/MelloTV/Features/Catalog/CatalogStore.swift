import Foundation
import Observation
import MelloKit

@MainActor
@Observable
final class CatalogStore {
    private(set) var sections: [CatalogSection] = []
    private(set) var featuredTitles: [MediaTitle] = []
    private(set) var isLoading = false
    private(set) var hasLoaded = false
    private(set) var errorMessage: String?

    let api: MelloTVAPI
    let category: CatalogCategory
    var profileID: String?

    private var generation = 0
    private var activeTask: Task<Void, Never>?

    init(api: MelloTVAPI, category: CatalogCategory, profileID: String? = nil) {
        self.api = api
        self.category = category
        self.profileID = profileID
    }

    func updateProfileID(_ newID: String?) {
        guard profileID != newID else { return }
        profileID = newID
        activeTask?.cancel()
        sections = []
        featuredTitles = []
        hasLoaded = false
        errorMessage = nil
        Task {
            await load()
        }
    }

    func removeRecommendation(_ titleID: String) {
        sections = sections.compactMap { section in
            let titles = section.titles.filter { $0.id != titleID }
            return titles.isEmpty ? nil : CatalogSection(id: section.id, heading: section.heading, titles: titles, reason: section.reason)
        }
        featuredTitles.removeAll { $0.id == titleID }
    }

    func load(force: Bool = false) async {
        if !force && hasLoaded && !sections.isEmpty { return }

        activeTask?.cancel()
        generation += 1
        let token = generation
        let task = Task {
            isLoading = true
            errorMessage = nil
            defer {
                if token == generation { isLoading = false }
            }

            do {
                let response: CatalogResponse
                if let profileID {
                    try await api.waitForCatalogReady(profileID: profileID)
                    guard !Task.isCancelled else { return }
                    response = try await api.recommendationPage(category: category.mediaTypeFilter, profileID: profileID, topic: "all")
                } else {
                    response = try await api.home()
                }

                guard !Task.isCancelled else { return }

                guard response.configured else {
                    sections = []
                    featuredTitles = []
                    errorMessage = "TMDB isn't configured on this server."
                    hasLoaded = true
                    return
                }

                let loadedSections = response.sections ?? []
                self.sections = loadedSections

                // Keep the existing multi-card showroom and editorial spotlight.
                // The server's reserved pick leads, followed by the first ranked row.
                var featuredIDs = Set<String>()
                let candidates = (response.featured ?? []) +
                    (loadedSections.first(where: { !$0.titles.isEmpty })?.titles ?? response.titles)
                self.featuredTitles = candidates.filter { featuredIDs.insert($0.id).inserted }

                self.hasLoaded = true
            } catch is CancellationError {
                return
            } catch {
                guard !Task.isCancelled else { return }
                if sections.isEmpty { errorMessage = error.localizedDescription }
                hasLoaded = true
            }
        }
        activeTask = task
        await task.value
    }
}

enum CatalogCategory: String, CaseIterable, Identifiable, Equatable {
    case home
    case movies
    case tv

    var id: String { rawValue }

    var title: String {
        switch self {
        case .home: "Home"
        case .movies: "Movies"
        case .tv: "TV"
        }
    }

    var mediaTypeFilter: MediaType? {
        switch self {
        case .home: nil
        case .movies: .movie
        case .tv: .tv
        }
    }
}
