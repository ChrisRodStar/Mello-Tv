import SwiftUI
import OSLog

@main
struct MelloTVApp: App {
    @UIApplicationDelegateAdaptor(MelloTVAppDelegate.self) private var appDelegate

    var body: some Scene {
        WindowGroup {
            AppRootView()
        }
    }
}


@MainActor
final class MelloTVAppDelegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication,
                     didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        PlaybackTiming(action: .launch).record(.appDidFinishLaunching)
        return true
    }

    func application(_ application: UIApplication,
                     supportedInterfaceOrientationsFor window: UIWindow?) -> UIInterfaceOrientationMask {
        PlaybackOrientationPolicy.mask(for: window)
    }
}

// Restrict only the window presenting playback. Normal app rotation is restored
// after the last player owner leaves; episode swaps and menus do not release it.
@MainActor
enum PlaybackOrientationPolicy {
    private struct Lease {
        var owners: Set<UUID>
        let previousOrientation: UIInterfaceOrientation
    }
    private static var leases: [ObjectIdentifier: Lease] = [:]
    private static let logger = Logger(subsystem: Bundle.main.bundleIdentifier ?? "com.tinymellow.mellotv", category: "Orientation")

    static func mask(for window: UIWindow?) -> UIInterfaceOrientationMask {
        if let window, leases[ObjectIdentifier(window)] != nil { return .landscape }
        return window?.traitCollection.userInterfaceIdiom == .pad ? .all : .allButUpsideDown
    }

    static func acquire(window: UIWindow, owner: UUID) {
        let key = ObjectIdentifier(window)
        if var lease = leases[key] {
            lease.owners.insert(owner)
            leases[key] = lease
        } else {
            leases[key] = Lease(owners: [owner], previousOrientation: window.windowScene?.interfaceOrientation ?? .portrait)
        }
        request(.landscape, in: window)
    }

    static func release(window: UIWindow, owner: UUID) {
        let key = ObjectIdentifier(window)
        guard var lease = leases[key] else { return }
        lease.owners.remove(owner)
        guard lease.owners.isEmpty else { leases[key] = lease; return }
        leases.removeValue(forKey: key)
        let restored: UIInterfaceOrientationMask
        switch lease.previousOrientation {
        case .landscapeLeft: restored = .landscapeLeft
        case .landscapeRight: restored = .landscapeRight
        case .portraitUpsideDown where window.traitCollection.userInterfaceIdiom == .pad:
            restored = .portraitUpsideDown
        default: restored = .portrait
        }
        request(restored, in: window)
    }

    private static func request(_ orientations: UIInterfaceOrientationMask, in window: UIWindow) {
        var controller = window.rootViewController
        while let current = controller {
            current.setNeedsUpdateOfSupportedInterfaceOrientations()
            controller = current.presentedViewController
        }
        window.windowScene?.requestGeometryUpdate(.iOS(interfaceOrientations: orientations)) { error in
            logger.error("Orientation update failed: \(error.localizedDescription, privacy: .public)")
        }
    }
}

struct PlaybackLandscapeLock: UIViewControllerRepresentable {
    func makeUIViewController(context: Context) -> Controller { Controller() }
    func updateUIViewController(_ controller: Controller, context: Context) {}
    static func dismantleUIViewController(_ controller: Controller, coordinator: ()) { controller.releaseLock() }

    final class Controller: UIViewController {
        private let owner = UUID()
        private weak var lockedWindow: UIWindow?

        override func loadView() {
            view = UIView()
            view.isUserInteractionEnabled = false
            view.backgroundColor = .clear
        }

        override func viewDidAppear(_ animated: Bool) {
            super.viewDidAppear(animated)
            guard let window = view.window, lockedWindow !== window else { return }
            releaseLock()
            lockedWindow = window
            PlaybackOrientationPolicy.acquire(window: window, owner: owner)
        }

        func releaseLock() {
            if let lockedWindow { PlaybackOrientationPolicy.release(window: lockedWindow, owner: owner) }
            lockedWindow = nil
        }
    }
}
