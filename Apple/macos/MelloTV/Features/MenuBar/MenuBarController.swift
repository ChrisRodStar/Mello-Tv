import Cocoa

@MainActor
final class MenuBarController: NSObject, NSMenuDelegate {
    private let server: ServerController
    private let preferences: AppPreferences
    private let actions: AppActions
    private var port: Int { server.port }
    private var statusItem: NSStatusItem!
    private var menu: NSMenu!
    
    private var statusMenuItem: NSMenuItem!
    private var openBrowserMenuItem: NSMenuItem!
    private var toggleServerMenuItem: NSMenuItem!
    private var restartServerMenuItem: NSMenuItem!

    init(server: ServerController, preferences: AppPreferences, actions: AppActions) {
        self.server = server
        self.preferences = preferences
        self.actions = actions
        super.init()
    }

    func setVisible(_ visible: Bool) {
        if visible {
            if statusItem == nil { setup() }
            update(running: server.state.isRunning)
        } else if let statusItem {
            NSStatusBar.system.removeStatusItem(statusItem)
            self.statusItem = nil
        }
    }

    private func setup() {
        // Reserve the native icon-sized slot in the menu bar.
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        guard let button = statusItem.button else { return }
        
        let config = NSImage.SymbolConfiguration(pointSize: 14, weight: .semibold)
        if let image = NSImage(systemSymbolName: "play.tv.fill", accessibilityDescription: "Mello TV")?.withSymbolConfiguration(config) {
            image.isTemplate = true
            button.image = image
        } else if let fallback = NSImage(systemSymbolName: "tv", accessibilityDescription: "Mello TV") {
            fallback.isTemplate = true
            button.image = fallback
        } else {
            button.title = "▶ Mello TV"
        }
        
        button.toolTip = "Mello TV"
        
        buildMenu()
        statusItem.menu = menu
    }
    
    private func buildMenu() {
        menu = NSMenu()
        menu.autoenablesItems = false
        menu.delegate = self
        
        // A native informational row avoids replacing a self-sizing hosting view
        // while AppKit is connecting or laying out the status-item scene.
        statusMenuItem = NSMenuItem()
        statusMenuItem.isEnabled = false
        menu.addItem(statusMenuItem)
        
        menu.addItem(NSMenuItem.separator())
        
        // 2. Core App Navigation
        let dashboardItem = NSMenuItem(title: "Open Mello TV", action: #selector(openDashboardAction), keyEquivalent: "o")
        dashboardItem.target = self
        dashboardItem.image = NSImage(systemSymbolName: "macwindow", accessibilityDescription: nil)
        menu.addItem(dashboardItem)

        openBrowserMenuItem = NSMenuItem(title: "Open Web Player…", action: #selector(openBrowserAction), keyEquivalent: "")
        openBrowserMenuItem.target = self
        openBrowserMenuItem.image = NSImage(systemSymbolName: "safari", accessibilityDescription: nil)
        menu.addItem(openBrowserMenuItem)
        
        menu.addItem(NSMenuItem.separator())
        
        // 3. Server Management
        restartServerMenuItem = NSMenuItem(title: "Restart Server", action: #selector(restartServerAction), keyEquivalent: "r")
        restartServerMenuItem.target = self
        restartServerMenuItem.image = NSImage(systemSymbolName: "arrow.clockwise", accessibilityDescription: nil)
        menu.addItem(restartServerMenuItem)
        
        toggleServerMenuItem = NSMenuItem(title: "Stop Server", action: #selector(toggleServerAction), keyEquivalent: "s")
        toggleServerMenuItem.target = self
        toggleServerMenuItem.image = NSImage(systemSymbolName: "power", accessibilityDescription: nil)
        menu.addItem(toggleServerMenuItem)
        
        menu.addItem(NSMenuItem.separator())
        
        // 4. Settings & Quit
        let settingsItem = NSMenuItem(title: "Settings…", action: #selector(openSettingsAction), keyEquivalent: ",")
        settingsItem.target = self
        settingsItem.image = NSImage(systemSymbolName: "gearshape", accessibilityDescription: nil)
        menu.addItem(settingsItem)
        
        let quitItem = NSMenuItem(title: "Quit Mello TV", action: #selector(quitAction), keyEquivalent: "q")
        quitItem.target = self
        menu.addItem(quitItem)
    }

    func update(running: Bool) {
        guard statusItem != nil else { return }
        
        statusMenuItem.title = running ? "Server running on port \(port)" : "Server stopped"
        let indicator = NSImage.SymbolConfiguration(paletteColors: [running ? .systemGreen : .systemOrange])
        statusMenuItem.image = NSImage(systemSymbolName: "circle.fill", accessibilityDescription: nil)?
            .withSymbolConfiguration(indicator)
        
        openBrowserMenuItem.isEnabled = running
        restartServerMenuItem.isEnabled = running
        toggleServerMenuItem.title = running ? "Stop Server" : "Start Server"
        toggleServerMenuItem.image = NSImage(systemSymbolName: running ? "pause.fill" : "play.fill", accessibilityDescription: nil)
        
        statusItem.button?.appearsDisabled = false
    }

    func menuWillOpen(_ menu: NSMenu) {
        update(running: server.state.isRunning)
    }

    @objc private func openSettingsAction() { actions.openSettings() }
    @objc private func openDashboardAction() { actions.openDashboard() }
    @objc private func openBrowserAction() { actions.openWebsite() }
    @objc private func restartServerAction() { actions.restartServer() }
    @objc private func toggleServerAction() { actions.toggleServer() }
    @objc private func quitAction() { actions.quit() }
}
