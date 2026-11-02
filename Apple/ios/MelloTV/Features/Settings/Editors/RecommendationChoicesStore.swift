import Foundation
import MelloKit
import Observation

@MainActor
@Observable
final class RecommendationChoicesStore {
    private let profileID: String
    private let api: MelloTVAPI

    private(set) var choices: RecommendationChoices?
    private(set) var error: String?
    private(set) var isLoading = false
    private(set) var isSaving = false

    init(profileID: String) {
        self.profileID = profileID
        api = MelloTVAPI(baseURL: ServerAddress.baseURL)
    }

    var categories: [RecommendationChoice] {
        [.favorites, .hidden, .watched, .ignored].filter { !titles(for: $0).isEmpty }
    }

    func titles(for category: RecommendationChoice) -> [MediaTitle] {
        choices?.titles(for: category) ?? []
    }

    func load() async {
        guard !isLoading, !isSaving else { return }
        isLoading = true
        defer { isLoading = false }
        do {
            choices = try await api.recommendationChoices(profileID: profileID)
            error = nil
        } catch {
            guard !Task.isCancelled else { return }
            self.error = "Couldn’t load your choices. Try again."
        }
    }

    /// Persist each change before removing it locally. Failed and unattempted titles stay selected.
    func remove(ids: Set<String>, from category: RecommendationChoice) async -> Set<String> {
        guard !isSaving, !isLoading else { return [] }
        let titles = titles(for: category).filter { ids.contains($0.id) }
        guard !titles.isEmpty else { return [] }
        isSaving = true
        error = nil
        defer { isSaving = false }
        var removed: Set<String> = []
        for title in titles {
            do {
                try await api.setRecommendationChoice(
                    profileID: profileID, title: title, choice: category, enabled: false)
                removeLocally(title, from: category)
                removed.insert(title.id)
                notifyChange(title, category: category)
            } catch {
                self.error =
                    removed.isEmpty
                    ? "Couldn’t remove your selection. Try again."
                    : "Some choices couldn’t be removed. They’re still selected; try again."
                break
            }
        }
        return removed
    }

    private func removeLocally(_ title: MediaTitle, from category: RecommendationChoice) {
        guard var updated = choices else { return }
        switch category {
        case .favorites: updated.favorites.removeAll { $0.id == title.id }
        case .hidden: updated.hidden.removeAll { $0.id == title.id }
        case .watched: updated.watched.removeAll { $0.id == title.id }
        case .ignored: updated.ignored.removeAll { $0.id == title.id }
        }
        choices = updated
    }

    private func notifyChange(_ title: MediaTitle, category: RecommendationChoice) {
        var info: [String: Any] = [
            "profileID": profileID, "titleID": title.id,
            "choice": category.rawValue, "enabled": false,
        ]
        if category == .favorites {
            info["favoriteTitle"] = title
            info["favoriteEnabled"] = false
        }
        NotificationCenter.default.post(
            name: .melloTVRecommendationsChanged, object: nil, userInfo: info)
    }
}
