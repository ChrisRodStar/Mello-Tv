import Cocoa

@MainActor
final class MelloTVAppDelegate: NSObject, NSApplicationDelegate {
    private let server = ServerController(port: 8080)
    private let loginItem = LoginItemService()
    private lazy var actions: AppActions = AppActions(
        openDashboard: { [weak self] in self?.mainWindow.present() },
        toggleServer: { [weak self] in self?.server.toggle() },
        restartServer: { [weak self] in self?.server.restart() },
        openWebsite: { [weak self] in self?.openWebsite() },
        openLogs: { [weak self] in self?.server.openLogs() },
        openData: { [weak self] in self?.server.openDataDirectory() },
        showWelcome: { [weak self] in self?.welcomeWindow.present() },
        quit: { [weak self] in self?.quit() }
    )
    private lazy var mainWindow: MainWindowController = MainWindowController(port: server.port, process: server.state, actions: actions)
    private lazy var welcomeWindow: WelcomeWindowController = WelcomeWindowController(port: server.port) { [weak self] in self?.mainWindow.present() }
    private lazy var menuBar: MenuBarController = MenuBarController(server: server, loginItem: loginItem, actions: actions)

    func applicationDidFinishLaunching(_ notification: Notification) {
        configureApplicationMenu()
        loginItem.migrateExistingPreference()
        menuBar.setup()
        server.onStateChange = { [weak self] running in
            self?.menuBar.update(running: running)
            self?.mainWindow.serverStateChanged()
        }
        server.start()
        presentMainWindow()
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag { presentMainWindow() }
        return true
    }

    func applicationWillTerminate(_ notification: Notification) { stopServer(waitTimeout: 3) }

    func stopServer(waitTimeout: TimeInterval) { server.stop(waitTimeout: waitTimeout) }

    private func presentMainWindow() {
        if UserDefaults.standard.bool(forKey: WelcomeWindowController.completedKey) { mainWindow.present() }
        else { welcomeWindow.present() }
    }

    private func openWebsite() { NSWorkspace.shared.open(AppLinks.clientURL(port: server.port)) }

    @objc private func quit() {
        server.stop(waitTimeout: 2)
        NSApp.terminate(nil)
    }

    @objc private func showOverview() { mainWindow.present(destination: .overview) }
    @objc private func showProviders() { mainWindow.present(destination: .providers) }
    @objc private func showNetwork() { mainWindow.present(destination: .network) }
    @objc private func showUpdates() { mainWindow.present(destination: .updates) }
    @objc private func toggleSidebar() { mainWindow.toggleSidebar() }
    @objc private func refreshStatus() { mainWindow.present() }

    private func configureApplicationMenu() {
        let main = NSMenu()
        let appItem = NSMenuItem()
        let appMenu = NSMenu(title: "Mello TV")
        appMenu.addItem(withTitle: "About Mello TV", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Hide Mello TV", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        appMenu.addItem(.separator())
        add("Quit Mello TV", action: #selector(quit), key: "q", to: appMenu)
        appItem.submenu = appMenu
        main.addItem(appItem)

        let viewItem = NSMenuItem()
        let viewMenu = NSMenu(title: "View")
        add("Overview", action: #selector(showOverview), key: "1", to: viewMenu)
        add("Providers", action: #selector(showProviders), key: "2", to: viewMenu)
        add("Network", action: #selector(showNetwork), key: "3", to: viewMenu)
        add("Updates", action: #selector(showUpdates), key: "4", to: viewMenu)
        viewMenu.addItem(.separator())
        let sidebar = add("Toggle Sidebar", action: #selector(toggleSidebar), key: "s", to: viewMenu)
        sidebar.keyEquivalentModifierMask = [.command, .control]
        add("Refresh Status", action: #selector(refreshStatus), key: "r", to: viewMenu)
        viewItem.submenu = viewMenu
        main.addItem(viewItem)

        let windowItem = NSMenuItem()
        let windowMenu = NSMenu(title: "Window")
        windowMenu.addItem(withTitle: "Close Window", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        windowMenu.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        windowItem.submenu = windowMenu
        main.addItem(windowItem)
        NSApp.mainMenu = main
        NSApp.windowsMenu = windowMenu
    }

    @discardableResult
    private func add(_ title: String, action: Selector, key: String, to menu: NSMenu) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.target = self
        menu.addItem(item)
        return item
    }
}
