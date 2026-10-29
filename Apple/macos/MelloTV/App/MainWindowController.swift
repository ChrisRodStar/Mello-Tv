import Cocoa
import SwiftUI

@MainActor
final class MainWindowController: NSWindowController, NSWindowDelegate, NSToolbarDelegate, NSToolbarItemValidation {
    private let model: ServerStatusStore
    private let process: ServerProcessState
    private let actions: AppActions
    private let navigation = DesktopNavigation()
    var onVisibilityChange: ((Bool) -> Void)?
    private var hasPresented = false
    private let sidebarID = NSToolbarItem.Identifier("MelloTVSidebar")
    private let refreshID = NSToolbarItem.Identifier("MelloTVRefresh")
    private let websiteID = NSToolbarItem.Identifier("MelloTVWebsite")
    private let logsID = NSToolbarItem.Identifier("MelloTVLogs")

    init(port: Int, process: ServerProcessState, actions: AppActions) {
        self.model = ServerStatusStore(port: port)
        self.process = process
        self.actions = actions
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
        let toolbar = NSToolbar(identifier: "MelloTVMainToolbar")
        toolbar.delegate = self
        toolbar.displayMode = .iconOnly
        toolbar.allowsUserCustomization = false
        window.toolbar = toolbar
        window.toolbarStyle = .unified
        window.contentViewController = WindowContentController(
            rootView: DesktopShellView(navigation: navigation, model: model, process: process, port: port, actions: actions)
        )
        window.setFrameAutosaveName("MelloTVDesktopWindow")
    }

    required init?(coder: NSCoder) { nil }

    func present(destination: DesktopDestination? = nil) {
        if let destination = destination { navigation.selection = destination }
        model.start()
        model.refreshNow()
        onVisibilityChange?(true)
        showWindow(nil)
        window?.makeKeyAndOrderFront(nil)
        if !hasPresented, let window {
            window.contentView?.layoutSubtreeIfNeeded()
            window.minSize = WindowSizing.mainFrame
            window.setFrame(NSRect(origin: window.frame.origin, size: WindowSizing.mainFrame), display: true)
            window.center()
            hasPresented = true
        }
        NSApp.activate(ignoringOtherApps: true)
    }

    func windowWillClose(_ notification: Notification) {
        model.stop()
        onVisibilityChange?(false)
    }

    func serverStateChanged() {
        if window?.isVisible == true { model.refreshNow() }
        window?.toolbar?.validateVisibleItems()
    }

    func windowWillResize(_ sender: NSWindow, to frameSize: NSSize) -> NSSize {
        WindowSizing.constrainedMainFrame(frameSize)
    }

    // MARK: - NSToolbarDelegate

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [sidebarID, .flexibleSpace, refreshID, websiteID, logsID]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [sidebarID, .flexibleSpace, refreshID, websiteID, logsID]
    }

    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier itemIdentifier: NSToolbarItem.Identifier, willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        switch itemIdentifier {
        case sidebarID:
            let item = NSToolbarItem(itemIdentifier: itemIdentifier)
            item.label = "Toggle sidebar"
            item.paletteLabel = "Toggle sidebar"
            item.toolTip = "Toggle sidebar"
            item.image = NSImage(systemSymbolName: "sidebar.left", accessibilityDescription: "Toggle sidebar")
            item.target = self
            item.action = #selector(toggleSidebarAction)
            return item
        case refreshID:
            let item = NSToolbarItem(itemIdentifier: itemIdentifier)
            item.label = "Refresh"
            item.paletteLabel = "Refresh status"
            item.toolTip = "Refresh server status"
            item.image = NSImage(systemSymbolName: "arrow.clockwise", accessibilityDescription: "Refresh status")
            item.target = self
            item.action = #selector(refreshAction)
            return item
        case websiteID:
            let item = NSToolbarItem(itemIdentifier: itemIdentifier)
            item.label = "Website"
            item.paletteLabel = "Open website"
            item.toolTip = "Open web dashboard"
            item.image = NSImage(systemSymbolName: "safari", accessibilityDescription: "Open web dashboard")
            item.target = self
            item.action = #selector(openWebsiteAction)
            return item
        case logsID:
            let item = NSToolbarItem(itemIdentifier: itemIdentifier)
            item.label = "Logs"
            item.paletteLabel = "Open logs"
            item.toolTip = "Open server log file"
            item.image = NSImage(systemSymbolName: "doc.text", accessibilityDescription: "Open server log file")
            item.target = self
            item.action = #selector(openLogsAction)
            return item
        default:
            return nil
        }
    }

    func validateToolbarItem(_ item: NSToolbarItem) -> Bool {
        if item.itemIdentifier == refreshID { return !model.isRefreshing }
        if item.itemIdentifier == websiteID { return process.isRunning && model.isReachable }
        return true
    }

    @objc func toggleSidebar() {
        navigation.isSidebarExpanded.toggle()
    }

    @objc private func toggleSidebarAction() {
        toggleSidebar()
    }

    @objc private func refreshAction() {
        model.refreshNow()
    }

    @objc private func openWebsiteAction() {
        actions.openWebsite()
    }

    @objc private func openLogsAction() {
        actions.openLogs()
    }
}
