import Foundation
import SwiftUI
import Combine
import MelloKit

@MainActor
public final class ProfileStore: ObservableObject {
    public static let shared = ProfileStore()

    private let api: MelloTVAPI
    private var cancellables = Set<AnyCancellable>()

    @Published public var profiles: [Profile] = []
    @Published public var activeProfile: Profile?
    @Published public var isLoading: Bool = false
    @Published public var errorMessage: String? = nil

    public var activeProfileID: String? {
        activeProfile?.id ?? profiles.first?.id
    }

    public init(api: MelloTVAPI = .shared) {
        self.api = api

        // Auto-load profiles on initialize
        Task { [weak self] in
            await self?.loadProfiles()
        }

        // Auto-load profiles on server connect
        ServerAddress.shared.$activeBaseURL
            .dropFirst()
            .removeDuplicates()
            .sink { [weak self] url in
                guard url != nil else { return }
                Task { [weak self] in
                    await self?.loadProfiles()
                }
            }
            .store(in: &cancellables)

        ServerAddress.shared.$isConnected
            .dropFirst()
            .removeDuplicates()
            .filter { $0 }
            .sink { [weak self] _ in
                Task { [weak self] in
                    await self?.loadProfiles()
                }
            }
            .store(in: &cancellables)
    }

    private var persistenceIdentityURL: URL {
        ServerAddress.serverIdentityURL
    }

    public func loadProfiles() async {
        guard !isLoading else { return }
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            let loaded = try await api.profiles()
            self.profiles = loaded

            let savedID = (ServerAddress.currentBaseURL.flatMap { ProfilePersistence.activeProfileID(for: $0) })
                ?? ProfilePersistence.activeProfileID(for: persistenceIdentityURL)
                ?? ProfilePersistence.activeProfileID(for: ServerAddress.baseURL)

            if let savedID, savedID != "default", let match = loaded.first(where: { $0.id == savedID }) {
                self.activeProfile = match
                if let base = ServerAddress.currentBaseURL {
                    ProfilePersistence.setActiveProfileID(match.id, for: base)
                }
                ProfilePersistence.setActiveProfileID(match.id, for: persistenceIdentityURL)
                ProfilePersistence.setActiveProfileID(match.id, for: ServerAddress.baseURL)
            } else if let first = loaded.first {
                selectProfile(first)
            } else {
                self.activeProfile = nil
            }
        } catch {
            self.errorMessage = error.localizedDescription
        }
    }

    public func selectProfile(_ profile: Profile) {
        self.activeProfile = profile
        if let base = ServerAddress.currentBaseURL {
            ProfilePersistence.setActiveProfileID(profile.id, for: base)
        }
        ProfilePersistence.setActiveProfileID(profile.id, for: persistenceIdentityURL)
        ProfilePersistence.setActiveProfileID(profile.id, for: ServerAddress.baseURL)
        Task {
            await api.touchActivity(profileID: profile.id)
        }
        NotificationCenter.default.post(name: Notification.Name("MelloTVActiveProfileChanged"), object: profile.id)
    }

    public func switchProfile() {
        self.activeProfile = nil
        if let base = ServerAddress.currentBaseURL {
            ProfilePersistence.setActiveProfileID(nil, for: base)
        }
        ProfilePersistence.setActiveProfileID(nil, for: persistenceIdentityURL)
        ProfilePersistence.setActiveProfileID(nil, for: ServerAddress.baseURL)
        NotificationCenter.default.post(name: Notification.Name("MelloTVActiveProfileChanged"), object: nil)
    }
}
