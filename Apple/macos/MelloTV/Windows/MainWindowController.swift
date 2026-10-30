import Cocoa
import SwiftUI
import MelloKit

@MainActor
final class MainWindowController: NSWindowController, NSWindowDelegate {
    private let model: ServerStatusStore
    private let navigation = DesktopNavigation()
    var onVisibilityChange: ((Bool) -> Void)?
    private var hasPresented = false

    init(port: Int, process: ServerProcessState, actions: AppActions) {
        self.model = ServerStatusStore(port: port, isServerRunning: process.isRunning)
        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: WindowSizing.mainFrame),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = "Mello TV"
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        window.backgroundColor = MelloTVTheme.workspaceColor
        window.isReleasedWhenClosed = false
        super.init(window: window)
        window.delegate = self
        let sidebarButton = NSButton(
            image: NSImage(systemSymbolName: "sidebar.left", accessibilityDescription: "Toggle sidebar")!,
            target: self,
            action: #selector(toggleSidebar)
        )
        sidebarButton.isBordered = false
        sidebarButton.bezelStyle = .regularSquare
        sidebarButton.contentTintColor = .secondaryLabelColor
        sidebarButton.toolTip = "Toggle sidebar (⌘S)"
        sidebarButton.setAccessibilityLabel("Toggle sidebar")
        sidebarButton.frame = NSRect(x: 6, y: 0, width: 32, height: 28)

        // A borderless title-bar accessory keeps this control out of the
        // system toolbar's automatic glass background.
        let accessory = SidebarTitlebarAccessoryViewController(button: sidebarButton)
        accessory.layoutAttribute = .left
        window.addTitlebarAccessoryViewController(accessory)
        window.contentViewController = WindowContentController(
            rootView: DesktopShellView(navigation: navigation, model: model, process: process, port: port, actions: actions)
        )
        window.setContentSize(WindowSizing.mainFrame)
        window.minSize = WindowSizing.mainFrame
        window.contentMinSize = WindowSizing.mainFrame
        window.center()
    }

    required init?(coder: NSCoder) { nil }

    func present(destination: DesktopDestination? = nil) {
        if let destination = destination { navigation.selection = destination }
        model.start()
        model.refreshNow()
        onVisibilityChange?(true)
        if let window {
            window.minSize = WindowSizing.mainFrame
            window.contentMinSize = WindowSizing.mainFrame
            if !hasPresented {
                window.setContentSize(WindowSizing.mainFrame)
                window.center()
                hasPresented = true
            } else if window.frame.width < WindowSizing.mainFrame.width || window.frame.height < WindowSizing.mainFrame.height {
                window.setFrame(NSRect(origin: window.frame.origin, size: WindowSizing.constrainedMainFrame(window.frame.size)), display: true)
            }
        }
        showWindow(nil)
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func windowWillClose(_ notification: Notification) {
        model.stop()
        onVisibilityChange?(false)
    }

    func serverStateChanged(isRunning: Bool) {
        model.serverStateChanged(isRunning: isRunning)
        if window?.isVisible == true { model.refreshNow() }
    }

    func windowWillResize(_ sender: NSWindow, to frameSize: NSSize) -> NSSize {
        WindowSizing.constrainedMainFrame(frameSize)
    }

    @objc func toggleSidebar() {
        navigation.isSidebarExpanded.toggle()
    }
}

private final class SidebarTitlebarAccessoryViewController: NSTitlebarAccessoryViewController {
    private let button: NSButton

    init(button: NSButton) {
        self.button = button
        super.init(nibName: nil, bundle: nil)
        view = NSView(frame: NSRect(x: 0, y: 0, width: 44, height: 28))
        view.addSubview(button)
    }

    required init?(coder: NSCoder) { nil }

    override func viewDidLayout() {
        super.viewDidLayout()
        guard let closeButton = view.window?.standardWindowButton(.closeButton) else { return }
        // Match the native controls rather than the accessory's own center.
        let nativeFrame = view.convert(closeButton.bounds, from: closeButton)
        button.setFrameOrigin(NSPoint(x: button.frame.minX, y: nativeFrame.midY - button.frame.height / 2))
    }
}
