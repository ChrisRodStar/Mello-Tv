import Combine
import Foundation
import MelloKit

/// Exactly one mode is selected, so the app always has a way to reopen its window.
enum AppPresentation: String, CaseIterable, Identifiable {
    case dock
    case menuBar
    case both

    var id: Self { self }
    var title: String {
        switch self {
        case .dock: return "Dock"
        case .menuBar: return "Menu bar"
        case .both: return "Dock & Menu bar"
        }
    }
    var showsDock: Bool { self != .menuBar }
    var showsMenuBar: Bool { self != .dock }
}

@MainActor
final class AppPreferences: ObservableObject {
    private static let presentationKey = "mellotv.native.presentation"
    private static let appearanceKey = "app_appearance_mode"
    private let loginItem = LoginItemService()
    @Published private(set) var presentation: AppPresentation
    @Published private(set) var appearance: AppearanceMode
    @Published private(set) var launchAtLogin: Bool
    @Published private(set) var presentationError: String?
    var applyPresentation: ((AppPresentation) -> Bool)?

    init() {
        presentation = UserDefaults.standard.string(forKey: Self.presentationKey)
            .flatMap(AppPresentation.init(rawValue:)) ?? .both
        appearance = UserDefaults.standard.string(forKey: Self.appearanceKey)
            .flatMap(AppearanceMode.init(rawValue:)) ?? .system
        loginItem.migrateExistingPreference()
        launchAtLogin = loginItem.isEnabled
    }

    func setPresentation(_ value: AppPresentation) {
        guard applyPresentation?(value) == true else {
            presentationError = "Couldn't change where Mello TV appears. Please try again."
            return
        }
        presentationError = nil
        presentation = value
        UserDefaults.standard.set(value.rawValue, forKey: Self.presentationKey)
    }

    func setAppearance(_ mode: AppearanceMode) {
        appearance = mode
        UserDefaults.standard.set(mode.rawValue, forKey: Self.appearanceKey)
    }

    func setLaunchAtLogin(_ enabled: Bool) {
        loginItem.setEnabled(enabled)
        launchAtLogin = loginItem.isEnabled
    }
}
