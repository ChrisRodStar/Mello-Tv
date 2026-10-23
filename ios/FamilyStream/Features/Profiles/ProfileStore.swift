import Foundation
import SwiftUI

@MainActor
@Observable
final class ProfileStore {
    private let api: FamilyStreamAPI
    private let serverURL: URL

    var profiles: [Profile] = []
    var activeProfile: Profile?
    var isLoading = false
    var errorMessage: String?
    var isManagingProfiles = false

    var showProfileEditor = false
    var editingProfile: Profile?
    var showOnboarding = false

    init(api: FamilyStreamAPI? = nil) {
        let baseURL = ServerAddress.baseURL
        self.serverURL = baseURL
        self.api = api ?? FamilyStreamAPI(baseURL: baseURL)
    }

    func loadProfiles() async {
        isLoading = true
        errorMessage = nil
        do {
            let loaded = try await api.profiles()
            self.profiles = loaded
            isLoading = false

            // Restore active profile from persistence
            if let savedID = ProfilePersistence.activeProfileID(for: serverURL),
               let match = loaded.first(where: { $0.id == savedID }) {
                self.activeProfile = match
                checkOnboarding(for: match)
            } else if let first = loaded.first, loaded.count == 1 {
                // If only 1 profile exists and nothing saved, select it
                selectProfile(first)
            } else {
                // Multiple or no profiles saved, active profile remains nil (Who's watching shown)
                self.activeProfile = nil
            }
        } catch {
            isLoading = false
            errorMessage = error.localizedDescription
        }
    }

    // Keep the preferences editor in sync with Like actions made in details.
    // This changes local presentation only after the API has saved the choice.
    func applyFavoriteChoice(profileID: String, title: MediaTitle, enabled: Bool) {
        guard let index = profiles.firstIndex(where: { $0.id == profileID }) else { return }
        var profile = profiles[index]
        profile.preferences.likedTitles.removeAll { $0.type == title.type && $0.id == title.tmdbID }
        if enabled { profile.preferences.likedTitles.append(LikedTitle(id: title.tmdbID, type: title.type, title: title.title)) }
        profiles[index] = profile
        if activeProfile?.id == profileID { activeProfile = profile }
        if editingProfile?.id == profileID { editingProfile = profile }
    }

    func selectProfile(_ profile: Profile) {
        self.activeProfile = profile
        ProfilePersistence.setActiveProfileID(profile.id, for: serverURL)
        Task {
            await api.touchActivity(profileID: profile.id)
        }
        checkOnboarding(for: profile)
    }

    func checkOnboarding(for profile: Profile) {
        if !profile.preferences.completed {
            showOnboarding = true
        } else {
            showOnboarding = false
        }
    }

    func switchProfile() {
        self.activeProfile = nil
        ProfilePersistence.setActiveProfileID(nil, for: serverURL)
    }

    func openCreateProfile() {
        editingProfile = nil
        showProfileEditor = true
    }

    func openEditProfile(_ profile: Profile) {
        editingProfile = profile
        showProfileEditor = true
    }

    func saveProfile(name: String, color: String, existingID: String?) async throws {
        let cleanName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !cleanName.isEmpty else { return }

        if let existingID, let existing = profiles.first(where: { $0.id == existingID }) {
            let updated = Profile(
                id: existing.id,
                name: cleanName,
                color: color,
                preferences: existing.preferences
            )
            let newProfiles = try await api.updateProfile(updated)
            self.profiles = newProfiles
            if activeProfile?.id == existingID {
                self.activeProfile = updated
            }
        } else {
            let newID = UUID().uuidString.lowercased().replacingOccurrences(of: "-", with: "").prefix(12)
            let newProfile = Profile(
                id: String(newID),
                name: cleanName,
                color: color,
                preferences: .empty
            )
            let previousIDs = Set(profiles.map(\.id))
            let newProfiles = try await api.createProfile(newProfile)
            self.profiles = newProfiles
            if let created = newProfiles.first(where: { !previousIDs.contains($0.id) }) {
                selectProfile(created)
            }
        }
        showProfileEditor = false
    }

    func deleteProfile(_ profile: Profile) async throws {
        guard profiles.count > 1 else { return }
        let newProfiles = try await api.deleteProfile(id: profile.id)
        self.profiles = newProfiles
        if activeProfile?.id == profile.id {
            switchProfile()
        }
        showProfileEditor = false
    }

    func completeOnboarding(with preferences: ProfilePreferences) async throws {
        guard let current = activeProfile else { return }
        var updated = current
        updated.preferences = preferences
        let newProfiles = try await api.updateProfile(updated)
        self.profiles = newProfiles
        self.activeProfile = updated
        showOnboarding = false
    }
}
