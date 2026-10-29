import Cocoa
import SwiftUI

/// The Mac companion owns onboarding. The viewing client stays in the browser.
@MainActor
final class WelcomeWindowController: NSWindowController, NSWindowDelegate {
    var onVisibilityChange: ((Bool) -> Void)?
    static let completedKey = "mellotv.native.onboarding.completed"
    private let port: Int
    private let onCompleted: () -> Void

    init(port: Int, onCompleted: @escaping () -> Void) {
        self.port = port
        self.onCompleted = onCompleted
        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: WindowSizing.welcomeContent),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = "Welcome to Mello TV"
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        window.backgroundColor = MelloTVTheme.workspaceColor
        window.isMovableByWindowBackground = true
        window.contentMinSize = WindowSizing.welcomeMinimumContent
        window.isReleasedWhenClosed = false
        window.center()
        super.init(window: window)
        window.delegate = self
    }

    required init?(coder: NSCoder) { nil }

    func windowWillClose(_ notification: Notification) {
        onVisibilityChange?(false)
    }

    func present() {
        window?.contentViewController = WindowContentController(rootView: WelcomeOnboardingView(port: port) { [weak self] in
            guard let self = self else { return }
            UserDefaults.standard.set(true, forKey: Self.completedKey)
            self.close()
            self.onCompleted()
        })
        onVisibilityChange?(true)
        showWindow(nil)
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
}
