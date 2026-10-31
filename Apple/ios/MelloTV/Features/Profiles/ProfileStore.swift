import Foundation
import SwiftUI
import MelloKit

@MainActor
@Observable
final class ProfileStore {
    private let api: MelloTVAPI
    private var pendingCreation: (name: String, color: String, knownIDs: Set<String>)?

    var profiles: [Profile] = []
    var activeProfile: Profile?
    var isLoading = false
    var isDiscoveringServer = false
    var errorMessage: String?
    var isManagingProfiles = false

    var showProfileEditor = false
    var editingProfile: Profile?
    var showOnboarding = false
    var showServerConnectionSheet = false
    var discoveredServers: [DiscoveredServer] = []
    var connectedServerName: String = ServerAddress.serverName
    var connectedServerAddress: String = ServerAddress.baseURL.absoluteString

    init(api: MelloTVAPI? = nil) {
        let baseURL = ServerAddress.baseURL
        self.api = api ?? MelloTVAPI(baseURL: baseURL)
    }

    private var persistenceIdentityURL: URL {
        ServerAddress.serverIdentityURL
    }

    private func refreshServerMetadata() {
        connectedServerName = ServerAddress.serverName
        connectedServerAddress = ServerAddress.baseURL.absoluteString
    }

    func loadProfiles() async {
        guard !isLoading else { return }
        isLoading = true
        errorMessage = nil
        defer { isLoading = false; isDiscoveringServer = false }
        if ServerAddress.fallbackURLs.isEmpty {
            isDiscoveringServer = true
            if let discovered = await ServerAddress.ensureResolved() {
                discoveredServers = [discovered]
            }
            isDiscoveringServer = false
        }
        refreshServerMetadata()
        do {
            let loaded = try await api.profiles()
            try Task.checkCancellation()
            acceptProfiles(loaded)
        } catch {
            guard !Task.isCancelled else { return }
            // The API already attempts endpoint recovery. Do not recursively
            // discover and load again when a reachable server returns an error.
            errorMessage = error.localizedDescription
        }
    }

    private func acceptProfiles(_ loaded: [Profile]) {
        refreshServerMetadata()
        profiles = loaded
        errorMessage = nil
        let savedID = ProfilePersistence.activeProfileID(for: persistenceIdentityURL)
            ?? ProfilePersistence.activeProfileID(for: ServerAddress.baseURL)
        if let savedID, let match = loaded.first(where: { $0.id == savedID }) {
            activeProfile = match
            ProfilePersistence.setActiveProfileID(match.id, for: persistenceIdentityURL)
            checkOnboarding(for: match)
        } else if loaded.count == 1, let first = loaded.first {
            selectProfile(first)
        } else {
            activeProfile = nil
        }
    }

    func scanForServers() async {
        isDiscoveringServer = true
        let found = await ServerAddress.discoverServers()
        discoveredServers = found
        isDiscoveringServer = false
        if let first = found.first, profiles.isEmpty {
            ServerAddress.saveDiscoveredServer(first)
            refreshServerMetadata()
            await loadProfiles()
        }
    }

    func selectDiscoveredServer(_ server: DiscoveredServer) async {
        ServerAddress.saveDiscoveredServer(server)
        refreshServerMetadata()
        showServerConnectionSheet = false
        await loadProfiles()
    }

    func connectToCustomServer(_ rawInput: String) async throws {
        let server = try await ServerAddress.connectManually(to: rawInput)
        if !discoveredServers.contains(where: { $0.id == server.id }) {
            discoveredServers.insert(server, at: 0)
        }
        refreshServerMetadata()
        showServerConnectionSheet = false
        await loadProfiles()
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
        ProfilePersistence.setActiveProfileID(profile.id, for: persistenceIdentityURL)
        ProfilePersistence.setActiveProfileID(profile.id, for: ServerAddress.baseURL)
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
        ProfilePersistence.setActiveProfileID(nil, for: persistenceIdentityURL)
        ProfilePersistence.setActiveProfileID(nil, for: ServerAddress.baseURL)
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
            let newProfiles = try await api.updateProfileIdentity(updated)
            self.profiles = newProfiles
            if activeProfile?.id == existingID {
                self.activeProfile = newProfiles.first(where: { $0.id == existingID })
            }
        } else {
            let newID = UUID().uuidString.lowercased().replacingOccurrences(of: "-", with: "").prefix(12)
            let newProfile = Profile(
                id: String(newID),
                name: cleanName,
                color: color,
                preferences: .empty
            )
            if let pending = pendingCreation, pending.name == cleanName, pending.color == color {
                // A lost POST response may already have created the profile.
                // Reconcile a fresh list before allowing another explicit Save.
                let refreshed = try await api.profiles()
                let matches = refreshed.filter {
                    !pending.knownIDs.contains($0.id) && $0.name == cleanName && $0.color == color
                }
                guard matches.count <= 1 else {
                    throw APIError.serverMessage("Refresh your profiles to review the completed request.")
                }
                profiles = refreshed
                if let created = matches.first {
                    pendingCreation = nil
                    selectProfile(created)
                    showProfileEditor = false
                    return
                }
            }
            pendingCreation = (cleanName, color, Set(profiles.map(\.id)))
            let created = try await api.createProfile(newProfile)
            pendingCreation = nil
            profiles.removeAll { $0.id == created.id }
            profiles.append(created)
            selectProfile(created)
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

    func completeOnboarding(with preferences: ProfilePreferences, original: Profile) async throws {
        guard let current = activeProfile, current.id == original.id else { return }
        let latest = try await api.profileForPreferenceEditing(id: original.id)
        func matches(_ a: ProfilePreferences, _ b: ProfilePreferences) -> Bool {
            a.format == b.format && Set(a.genres) == Set(b.genres) && a.completed == b.completed && Set(a.likedTitles) == Set(b.likedTitles)
        }
        guard matches(latest.preferences, original.preferences) || matches(latest.preferences, preferences) else {
            throw APIError.serverMessage("Preferences changed on another device. Close and reopen this editor to review them.")
        }
        var updated = current
        updated.preferences = preferences
        let newProfiles = matches(latest.preferences, preferences) ? try await api.profiles() : try await api.updateProfile(updated)
        self.profiles = newProfiles
        if activeProfile?.id == current.id {
            self.activeProfile = newProfiles.first { $0.id == current.id }
        }
        try await api.onboardingGenerate(profileID: current.id)
        if activeProfile?.id == current.id { showOnboarding = false }
    }
}
