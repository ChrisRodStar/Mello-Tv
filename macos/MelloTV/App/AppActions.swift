struct AppActions {
    let preferences: AppPreferences
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
