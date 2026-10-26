import Cocoa

@MainActor
final class MenuBarController: NSObject {
    private let server: ServerController
    private let loginItem: LoginItemService
    private let actions: AppActions
    private var port: Int { server.port }
    private var statusItem: NSStatusItem!
    private var menu: NSMenu!
    
    private var statusMenuItem: NSMenuItem!
    private var addressMenuItem: NSMenuItem!
    private var copyAddressMenuItem: NSMenuItem!
    private var openBrowserMenuItem: NSMenuItem!
    private var toggleServerMenuItem: NSMenuItem!
    private var restartServerMenuItem: NSMenuItem!
    private var launchAtLoginMenuItem: NSMenuItem!
    

    init(server: ServerController, loginItem: LoginItemService, actions: AppActions) {
        self.server = server
        self.loginItem = loginItem
        self.actions = actions
        super.init()
    }

    func setup() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        guard let button = statusItem.button else { return }
        
        let config = NSImage.SymbolConfiguration(pointSize: 15, weight: .medium)
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
        
        // App header
        let titleItem = NSMenuItem(title: "Mello TV", action: nil, keyEquivalent: "")
        titleItem.isEnabled = false
        let boldFont = NSFont.boldSystemFont(ofSize: 13)
        titleItem.attributedTitle = NSAttributedString(string: "Mello TV", attributes: [.font: boldFont])
        menu.addItem(titleItem)
        
        // Status indicator
        statusMenuItem = NSMenuItem(title: "● Starting...", action: nil, keyEquivalent: "")
        statusMenuItem.isEnabled = false
        menu.addItem(statusMenuItem)
        
        addressMenuItem = NSMenuItem(title: "Local: \(server.primaryServerAddress())", action: nil, keyEquivalent: "")
        addressMenuItem.isEnabled = false
        menu.addItem(addressMenuItem)
        
        menu.addItem(NSMenuItem.separator())
        
        // Main actions
        let dashboardItem = NSMenuItem(title: "Open Dashboard", action: #selector(openDashboardAction), keyEquivalent: "o")
        dashboardItem.target = self
        menu.addItem(dashboardItem)

        openBrowserMenuItem = NSMenuItem(title: "Open Website…", action: #selector(openBrowserAction), keyEquivalent: "")
        openBrowserMenuItem.target = self
        menu.addItem(openBrowserMenuItem)
        
        copyAddressMenuItem = NSMenuItem(title: "Copy Server Address", action: #selector(copyServerAddressAction), keyEquivalent: "c")
        copyAddressMenuItem.target = self
        menu.addItem(copyAddressMenuItem)
        
        restartServerMenuItem = NSMenuItem(title: "Restart Server", action: #selector(restartServerAction), keyEquivalent: "r")
        restartServerMenuItem.target = self
        menu.addItem(restartServerMenuItem)
        
        toggleServerMenuItem = NSMenuItem(title: "Stop Server", action: #selector(toggleServerAction), keyEquivalent: "s")
        toggleServerMenuItem.target = self
        menu.addItem(toggleServerMenuItem)
        
        menu.addItem(NSMenuItem.separator())
        
        // Diagnostics & Utilities
        let openDataFolderItem = NSMenuItem(title: "Open Data Directory", action: #selector(openDataDirectoryAction), keyEquivalent: "d")
        openDataFolderItem.target = self
        menu.addItem(openDataFolderItem)
        
        let viewLogsItem = NSMenuItem(title: "View Server Logs", action: #selector(viewLogsAction), keyEquivalent: "l")
        viewLogsItem.target = self
        menu.addItem(viewLogsItem)
        
        menu.addItem(NSMenuItem.separator())
        
        let welcomeItem = NSMenuItem(title: "Welcome Guide…", action: #selector(openWelcomeGuideAction), keyEquivalent: "")
        welcomeItem.target = self
        menu.addItem(welcomeItem)

        let supportItem = NSMenuItem(title: "☕ Buy Me a Coffee", action: #selector(supportCreatorAction), keyEquivalent: "")
        supportItem.target = self
        menu.addItem(supportItem)
        menu.addItem(NSMenuItem.separator())

        // Launch at login
        launchAtLoginMenuItem = NSMenuItem(title: "Launch at Login", action: #selector(toggleLaunchAtLoginAction), keyEquivalent: "")
        launchAtLoginMenuItem.target = self
        launchAtLoginMenuItem.state = loginItem.isEnabled ? .on : .off
        menu.addItem(launchAtLoginMenuItem)
        
        menu.addItem(NSMenuItem.separator())
        
        // Quit
        let quitItem = NSMenuItem(title: "Quit Mello TV", action: #selector(quitAction), keyEquivalent: "q")
        quitItem.target = self
        menu.addItem(quitItem)
    }
    

    func update(running: Bool) {
        statusMenuItem.title = running ? "● Running on port \(port)" : "○ Server stopped"
        addressMenuItem.title = running ? "Network: \(server.primaryServerAddress())" : "Network: Offline"
        openBrowserMenuItem.isEnabled = running
        copyAddressMenuItem.isEnabled = running
        restartServerMenuItem.isEnabled = running
        toggleServerMenuItem.title = running ? "Stop Server" : "Start Server"
        statusItem.button?.appearsDisabled = false
    }

    @objc private func openDashboardAction() { actions.openDashboard() }
    @objc private func openBrowserAction() { actions.openWebsite() }
    @objc private func openWelcomeGuideAction() { actions.showWelcome() }
    @objc private func restartServerAction() { actions.restartServer() }
    @objc private func toggleServerAction() { actions.toggleServer() }
    @objc private func openDataDirectoryAction() { actions.openData() }
    @objc private func viewLogsAction() { actions.openLogs() }
    @objc private func quitAction() { actions.quit() }
    @objc private func copyServerAddressAction() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(server.primaryServerAddress(), forType: .string)
    }
    @objc private func supportCreatorAction() { NSWorkspace.shared.open(AppLinks.buyMeACoffeeURL) }
    @objc private func toggleLaunchAtLoginAction() {
        loginItem.setEnabled(!loginItem.isEnabled)
        launchAtLoginMenuItem.state = loginItem.isEnabled ? .on : .off
    }
}
