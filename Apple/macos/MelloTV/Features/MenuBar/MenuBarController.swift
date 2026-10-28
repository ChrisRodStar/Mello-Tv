import Cocoa
import SwiftUI

private struct MenuBarHeaderView: View {
    let isRunning: Bool
    let port: Int

    var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(isRunning ? Color.green : Color.orange)
                .frame(width: 8, height: 8)
            
            Text(isRunning ? "Streaming Active" : "Server Paused")
                .font(.system(size: 13, weight: .semibold))
                .foregroundColor(.primary)
            
            Spacer()
            
            if isRunning {
                Text(":\(port)")
                    .font(.system(size: 11, weight: .medium, design: .monospaced))
                    .foregroundColor(.secondary)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.secondary.opacity(0.12))
                    .clipShape(Capsule())
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 4)
        .frame(minWidth: 220, alignment: .leading)
    }
}

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
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
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
        
        // 1. Live Server Status (Color-coded indicator via custom view)
        statusMenuItem = NSMenuItem()
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
        
        let headerView = MenuBarHeaderView(isRunning: running, port: port)
        let hostingView = NSHostingView(rootView: headerView)
        hostingView.frame = NSRect(x: 0, y: 0, width: 220, height: 28)
        statusMenuItem.view = hostingView
        
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
