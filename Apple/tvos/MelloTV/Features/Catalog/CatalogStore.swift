import Foundation
import Observation
import MelloKit

@MainActor
@Observable
public final class CatalogStore {
    public private(set) var sections: [CatalogSection] = []
    public private(set) var featuredTitles: [MediaTitle] = []
    public private(set) var continueWatching: [WatchProgress] = []
    public private(set) var isLoading = false
    public private(set) var hasLoaded = false
    public private(set) var errorMessage: String?

    public let api: MelloTVAPI
    public let category: CatalogCategory
    public var profileID: String?

    private var generation = 0
    private var activeTask: Task<Void, Never>?

    public init(api: MelloTVAPI = .shared, category: CatalogCategory = .home, profileID: String? = nil) {
        self.api = api
        self.category = category
        self.profileID = profileID
    }

    public func updateProfileID(_ newID: String?) {
        guard profileID != newID else { return }
        profileID = newID
        activeTask?.cancel()
        sections = []
        featuredTitles = []
        continueWatching = []
        hasLoaded = false
        errorMessage = nil
        Task {
            await load()
        }
    }

    public func removeRecommendation(_ titleID: String) {
        sections = sections.compactMap { section in
            let titles = section.titles.filter { $0.id != titleID }
            return titles.isEmpty ? nil : CatalogSection(id: section.id, heading: section.heading, titles: titles, reason: section.reason)
        }
        featuredTitles.removeAll { $0.id == titleID }
        continueWatching.removeAll { $0.title.id == titleID }
    }

    public func load(force: Bool = false) async {
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
                    self.continueWatching = (try? await api.fetchContinueWatching(profileID: profileID)) ?? []
                } else {
                    switch category {
                    case .home:
                        response = try await api.home()
                    case .movies:
                        response = try await api.browse(.movie)
                    case .tv:
                        response = try await api.browse(.tv)
                    }
                    let effectivePID = profileID ?? ProfileStore.shared.activeProfileID
                    if let effectivePID, !effectivePID.isEmpty {
                        self.continueWatching = (try? await api.fetchContinueWatching(profileID: effectivePID)) ?? []
                    } else {
                        self.continueWatching = []
                    }
                }

                guard !Task.isCancelled else { return }

                guard response.configured else {
                    sections = []
                    featuredTitles = []
                    continueWatching = []
                    errorMessage = "TMDB isn't configured on this server."
                    hasLoaded = true
                    return
                }

                let loadedSections = response.sections ?? []
                self.sections = loadedSections

                // Keep the multi-card showroom and editorial spotlight.
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
