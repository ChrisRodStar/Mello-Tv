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
            styleMask: [.titled, .closable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = "Welcome to Mello TV"
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        window.backgroundColor = MelloTVTheme.workspaceColor
        window.isMovableByWindowBackground = true
        window.setContentSize(WindowSizing.welcomeContent)
        window.contentMinSize = WindowSizing.welcomeContent
        window.contentMaxSize = WindowSizing.welcomeContent
        window.isReleasedWhenClosed = false
        super.init(window: window)
        window.delegate = self
        window.contentViewController = WindowContentController(rootView: WelcomeOnboardingView(port: port) { [weak self] in
            guard let self = self else { return }
            UserDefaults.standard.set(true, forKey: Self.completedKey)
            self.close()
            self.onCompleted()
        })
        window.setContentSize(WindowSizing.welcomeContent)
        window.center()
    }

    required init?(coder: NSCoder) { nil }

    func windowWillClose(_ notification: Notification) {
        onVisibilityChange?(false)
    }

    func present() {
        window?.setContentSize(WindowSizing.welcomeContent)
        window?.center()
        onVisibilityChange?(true)
        showWindow(nil)
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
}
