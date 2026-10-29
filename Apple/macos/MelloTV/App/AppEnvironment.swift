import Foundation

/// Dependency container and action routing for Mello TV macOS.
struct AppEnvironment {
    let preferences: AppPreferences
    let updater: AppUpdateService
    let openSettings: () -> Void
    let openDashboard: () -> Void
    let toggleServer: () -> Void
    let restartServer: () -> Void
    let openWebsite: () -> Void
    let openLogs: () -> Void
    let openData: () -> Void
    let showWelcome: () -> Void
    let quit: () -> Void
}

typealias AppActions = AppEnvironment

