import Cocoa
import SwiftUI

@MainActor
final class MainWindowController: NSWindowController, NSWindowDelegate, NSToolbarDelegate, NSToolbarItemValidation {
    private let model: ServerStatusStore
    private let process: ServerProcessState
    private let actions: AppActions
    private let navigation = DesktopNavigation()
    private let sidebarID = NSToolbarItem.Identifier("MelloTVSidebar")
    private let refreshID = NSToolbarItem.Identifier("MelloTVRefresh")
    private let websiteID = NSToolbarItem.Identifier("MelloTVWebsite")
    private let logsID = NSToolbarItem.Identifier("MelloTVLogs")

    init(port: Int, process: ServerProcessState, actions: AppActions) {
        self.model = ServerStatusStore(port: port)
        self.process = process
        self.actions = actions
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1150, height: 750),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "Mello TV"
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        window.backgroundColor = MelloTVTheme.workspaceColor
        window.appearance = NSAppearance(named: .darkAqua)
        window.isReleasedWhenClosed = false
        super.init(window: window)
        window.delegate = self
        let toolbar = NSToolbar(identifier: "MelloTVMainToolbar")
        toolbar.delegate = self
        toolbar.displayMode = .iconOnly
        toolbar.allowsUserCustomization = false
        toolbar.showsBaselineSeparator = false
        window.toolbar = toolbar
        window.toolbarStyle = .unified
        let hostingController = NSHostingController(rootView: DesktopShellView(navigation: navigation, model: model, process: process, port: port, actions: actions))
        hostingController.sizingOptions = []
        window.contentViewController = hostingController
        let minWidth: CGFloat = 1150
        let minHeight: CGFloat = 750
        window.minSize = NSSize(width: minWidth, height: minHeight)
        window.contentMinSize = NSSize(width: minWidth, height: minHeight)
        if let contentView = window.contentView {
            NSLayoutConstraint.activate([
                contentView.widthAnchor.constraint(greaterThanOrEqualToConstant: minWidth),
                contentView.heightAnchor.constraint(greaterThanOrEqualToConstant: minHeight)
            ])
        }
        window.center()
        window.setFrameAutosaveName("MelloTVDesktopWindow")
        if window.frame.width < minWidth || window.frame.height < minHeight {
            window.setContentSize(NSSize(width: 1150, height: 750))
            window.center()
        }
    }

    required init?(coder: NSCoder) { nil }

    func windowWillResize(_ sender: NSWindow, to frameSize: NSSize) -> NSSize {
        NSSize(width: max(frameSize.width, 1150), height: max(frameSize.height, 750))
    }

    func present(destination: DesktopDestination? = nil) {
        if let destination = destination { navigation.selection = destination }
        model.start()
        model.refreshNow()
        showWindow(nil)
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func serverStateChanged() {
        if window?.isVisible == true { model.refreshNow() }
        window?.toolbar?.validateVisibleItems()
    }

    func windowWillClose(_ notification: Notification) { model.stop() }

    @objc func toggleSidebar() { navigation.isSidebarExpanded.toggle() }
    @objc private func refresh() { model.refreshNow() }
    @objc private func openWebsite() { actions.openWebsite() }
    @objc private func openLogs() { actions.openLogs() }

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [sidebarID, .flexibleSpace, refreshID, websiteID, logsID]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar)
    }

    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier identifier: NSToolbarItem.Identifier,
                 willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        let label: String
        let symbol: String
        let action: Selector
        switch identifier {
        case sidebarID: label = "Toggle sidebar"; symbol = "sidebar.left"; action = #selector(toggleSidebar)
        case refreshID: label = "Refresh status"; symbol = "arrow.clockwise"; action = #selector(refresh)
        case websiteID: label = "Open website"; symbol = "arrow.up.right.square"; action = #selector(openWebsite)
        case logsID: label = "View logs"; symbol = "doc.text"; action = #selector(openLogs)
        default: return nil
        }
        let item = NSToolbarItem(itemIdentifier: identifier)
        item.label = label
        item.toolTip = label
        item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
        item.isBordered = false
        item.target = self
        item.action = action
        return item
    }

    func validateToolbarItem(_ item: NSToolbarItem) -> Bool {
        if item.itemIdentifier == refreshID { return !model.isRefreshing }
        if item.itemIdentifier == websiteID { return process.isRunning && model.isReachable }
        return true
    }
}
