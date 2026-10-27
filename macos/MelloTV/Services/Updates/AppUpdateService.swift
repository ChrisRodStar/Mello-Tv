import Cocoa
import Combine
import Sparkle

@MainActor
final class AppUpdateService: NSObject, ObservableObject, SPUUpdaterDelegate {
    @Published private(set) var canCheckForUpdates = false
    @Published private(set) var automaticallyChecksForUpdates = false
    @Published private(set) var automaticallyDownloadsUpdates = false
    @Published private(set) var statusMessage = "Check for a newer version of Mello TV."
    @Published private(set) var isWaitingForPlayback = false
    private(set) var controller: SPUStandardUpdaterController!
    private let port: Int
    private let isServerRunning: () -> Bool
    private var installationTask: Task<Void, Never>?
    private var started = false
    private var installationRequested = false
    private var restartApproved = false
    private weak var terminatingApplication: NSApplication?

    init(port: Int, isServerRunning: @escaping () -> Bool) {
        self.port = port
        self.isServerRunning = isServerRunning
        super.init()
        controller = SPUStandardUpdaterController(startingUpdater: false, updaterDelegate: self, userDriverDelegate: nil)
        controller.updater.publisher(for: \.canCheckForUpdates).assign(to: &$canCheckForUpdates)
        controller.updater.publisher(for: \.automaticallyChecksForUpdates).assign(to: &$automaticallyChecksForUpdates)
        controller.updater.publisher(for: \.automaticallyDownloadsUpdates).assign(to: &$automaticallyDownloadsUpdates)
    }

    func start() {
        guard !started else { return }
        do {
            try controller.updater.start()
            started = true
        } catch {
            statusMessage = "Updates couldn’t start. Please try reopening Mello TV."
        }
    }

    func checkForUpdates() {
        guard canCheckForUpdates else { return }
        controller.checkForUpdates(nil)
    }

    func setAutomaticallyChecksForUpdates(_ enabled: Bool) {
        controller.updater.automaticallyChecksForUpdates = enabled
    }

    func setAutomaticallyDownloadsUpdates(_ enabled: Bool) {
        controller.updater.automaticallyDownloadsUpdates = enabled
    }

    func updater(_ updater: SPUUpdater, didFindValidUpdate item: SUAppcastItem) {
        statusMessage = "Mello TV \(item.displayVersionString) is available."
    }

    func updater(_ updater: SPUUpdater, willInstallUpdate item: SUAppcastItem) {
        installationRequested = true
    }

    func applicationShouldTerminate(_ application: NSApplication) -> NSApplication.TerminateReply {
        guard installationRequested, !restartApproved else { return .terminateNow }
        terminatingApplication = application
        waitForPlaybackBeforeInstalling { [weak self, weak application] in
            self?.terminatingApplication = nil
            application?.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }

    func updater(_ updater: SPUUpdater, shouldPostponeRelaunchForUpdate item: SUAppcastItem,
                 untilInvokingBlock installHandler: @escaping () -> Void) -> Bool {
        waitForPlaybackBeforeInstalling(installHandler)
        return true
    }

    func updater(_ updater: SPUUpdater, willInstallUpdateOnQuit item: SUAppcastItem,
                 immediateInstallationBlock immediateInstallHandler: @escaping () -> Void) -> Bool {
        waitForPlaybackBeforeInstalling(immediateInstallHandler)
        return true
    }

    func updaterWillRelaunchApplication(_ updater: SPUUpdater) {
        cancelInstallationWait()
    }

    func updater(_ updater: SPUUpdater, didAbortWithError error: Error) {
        cancelInstallationWait()
        installationRequested = false
        restartApproved = false
        terminatingApplication?.reply(toApplicationShouldTerminate: false)
        terminatingApplication = nil
    }

    func updater(_ updater: SPUUpdater, didFinishUpdateCycleFor updateCheck: SPUUpdateCheck, error: Error?) {
        guard let error else { return }
        cancelInstallationWait()
        let nsError = error as NSError
        if nsError.domain == SUSparkleErrorDomain && nsError.code == Int(SUError.noUpdateError.rawValue) {
            statusMessage = "You’re up to date."
        } else if nsError.domain == SUSparkleErrorDomain && nsError.code == Int(SUError.installationCanceledError.rawValue) {
            statusMessage = "Update installation was canceled."
        } else {
            statusMessage = "Couldn’t check for updates. Please try again later."
        }
    }

    private func waitForPlaybackBeforeInstalling(_ installHandler: @escaping () -> Void) {
        cancelInstallationWait()
        installationRequested = true
        restartApproved = false
        isWaitingForPlayback = true
        statusMessage = "Update ready. Waiting for streaming to finish before restarting."
        installationTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                if await self.canRestartServer() {
                    guard !Task.isCancelled else { return }
                    self.installationTask = nil
                    self.isWaitingForPlayback = false
                    self.restartApproved = true
                    self.statusMessage = "Installing update…"
                    installHandler()
                    return
                }
                do { try await Task.sleep(nanoseconds: 15_000_000_000) }
                catch { return }
            }
        }
    }

    private func canRestartServer() async -> Bool {
        await UpdateReadinessProbe(port: port).canRestartServer(isServerRunning: isServerRunning())
    }

    private func cancelInstallationWait() {
        installationTask?.cancel()
        installationTask = nil
        isWaitingForPlayback = false
    }
}

