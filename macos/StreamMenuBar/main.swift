import Cocoa
import Foundation

// MARK: - Process Supervisor & Menu Bar Controller

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var statusItem: NSStatusItem!
    private var menu: NSMenu!
    
    private var statusMenuItem: NSMenuItem!
    private var addressMenuItem: NSMenuItem!
    private var copyAddressMenuItem: NSMenuItem!
    private var openBrowserMenuItem: NSMenuItem!
    private var toggleServerMenuItem: NSMenuItem!
    private var restartServerMenuItem: NSMenuItem!
    private var launchAtLoginMenuItem: NSMenuItem!
    
    private var serverProcess: Process?
    private var bonjourService: NetService?
    private var logFileHandle: FileHandle?
    private var isRunning: Bool = false
    private let port: Int = 8080
    
    private var dataDirectoryURL: URL {
        let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        return appSupport.appendingPathComponent("Stream", isDirectory: true)
    }
    
    private var logsDirectoryURL: URL {
        let logs = FileManager.default.urls(for: .libraryDirectory, in: .userDomainMask).first!
        return logs.appendingPathComponent("Logs/Stream", isDirectory: true)
    }
    
    private var logFileURL: URL {
        logsDirectoryURL.appendingPathComponent("stream-server.log")
    }
    
    private var launchAgentPlistURL: URL {
        let lib = FileManager.default.urls(for: .libraryDirectory, in: .userDomainMask).first!
        return lib.appendingPathComponent("LaunchAgents/tv.stream.desktop.plist")
    }
    
    func applicationDidFinishLaunching(_ notification: Notification) {
        setupStatusItem()
        ensureDirectories()
        startServer()
    }
    
    func applicationWillTerminate(_ notification: Notification) {
        stopServer(waitTimeout: 3.0)
    }
    
    // MARK: - Status Bar UI Setup
    
    private func setupStatusItem() {
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
        
        addressMenuItem = NSMenuItem(title: "Local: \(primaryServerAddress())", action: nil, keyEquivalent: "")
        addressMenuItem.isEnabled = false
        menu.addItem(addressMenuItem)
        
        menu.addItem(NSMenuItem.separator())
        
        // Main actions
        openBrowserMenuItem = NSMenuItem(title: "Open Mello TV", action: #selector(openBrowserAction), keyEquivalent: "o")
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
        
        // Launch at login
        launchAtLoginMenuItem = NSMenuItem(title: "Launch at Login", action: #selector(toggleLaunchAtLoginAction), keyEquivalent: "")
        launchAtLoginMenuItem.target = self
        launchAtLoginMenuItem.state = isLaunchAtLoginEnabled() ? .on : .off
        menu.addItem(launchAtLoginMenuItem)
        
        menu.addItem(NSMenuItem.separator())
        
        // Quit
        let quitItem = NSMenuItem(title: "Quit Mello TV", action: #selector(quitAction), keyEquivalent: "q")
        quitItem.target = self
        menu.addItem(quitItem)
    }
    
    private func updateUIState(running: Bool) {
        DispatchQueue.main.async { [weak self] in
            guard let self = self else { return }
            self.isRunning = running
            
            if running {
                let greenAttributes: [NSAttributedString.Key: Any] = [
                    .foregroundColor: NSColor.systemGreen,
                    .font: NSFont.systemFont(ofSize: 12, weight: .medium)
                ]
                self.statusMenuItem.attributedTitle = NSAttributedString(string: "● Running on port \(self.port)", attributes: greenAttributes)
                self.addressMenuItem.title = "Network: \(self.primaryServerAddress())"
                self.openBrowserMenuItem.isEnabled = true
                self.copyAddressMenuItem.isEnabled = true
                self.restartServerMenuItem.isEnabled = true
                self.toggleServerMenuItem.title = "Stop Server"
                self.statusItem.button?.appearsDisabled = false
                self.startBonjourBroadcast()
            } else {
                let grayAttributes: [NSAttributedString.Key: Any] = [
                    .foregroundColor: NSColor.secondaryLabelColor,
                    .font: NSFont.systemFont(ofSize: 12)
                ]
                self.statusMenuItem.attributedTitle = NSAttributedString(string: "○ Server Stopped", attributes: grayAttributes)
                self.addressMenuItem.title = "Network: Offline"
                self.openBrowserMenuItem.isEnabled = false
                self.copyAddressMenuItem.isEnabled = false
                self.restartServerMenuItem.isEnabled = false
                self.toggleServerMenuItem.title = "Start Server"
                self.statusItem.button?.appearsDisabled = true
                self.stopBonjourBroadcast()
            }
        }
    }
    
    // MARK: - Server Process Management
    
    private func ensureDirectories() {
        try? FileManager.default.createDirectory(at: dataDirectoryURL.appendingPathComponent("data"), withIntermediateDirectories: true)
        try? FileManager.default.createDirectory(at: logsDirectoryURL, withIntermediateDirectories: true)
    }
    
    private func locateServerBinary() -> URL? {
        let bundleURL = Bundle.main.bundleURL
        
        // 1. Inside Stream.app/Contents/MacOS/stream-server
        let inAppBundle = bundleURL.appendingPathComponent("Contents/MacOS/stream-server")
        if FileManager.default.isExecutableFile(atPath: inAppBundle.path) {
            return inAppBundle
        }
        
        // 2. Next to executable
        if let exeDir = Bundle.main.executableURL?.deletingLastPathComponent() {
            let nextToExe = exeDir.appendingPathComponent("stream-server")
            if FileManager.default.isExecutableFile(atPath: nextToExe.path) {
                return nextToExe
            }
        }
        
        // 3. Current working directory / workspace development target
        let cwd = FileManager.default.currentDirectoryPath
        let releasePath = URL(fileURLWithPath: cwd).appendingPathComponent("target/release/stream-server")
        if FileManager.default.isExecutableFile(atPath: releasePath.path) {
            return releasePath
        }
        
        let debugPath = URL(fileURLWithPath: cwd).appendingPathComponent("target/debug/stream-server")
        if FileManager.default.isExecutableFile(atPath: debugPath.path) {
            return debugPath
        }
        
        return nil
    }
    
    private func locateWebDirectory() -> URL? {
        let bundleURL = Bundle.main.bundleURL
        
        // 1. Inside Stream.app/Contents/Resources/web
        let inAppResources = bundleURL.appendingPathComponent("Contents/Resources/web")
        if FileManager.default.fileExists(atPath: inAppResources.appendingPathComponent("index.html").path) {
            return inAppResources
        }
        
        // 2. Next to executable Resources
        if let resDir = Bundle.main.resourceURL?.appendingPathComponent("web") {
            if FileManager.default.fileExists(atPath: resDir.appendingPathComponent("index.html").path) {
                return resDir
            }
        }
        
        // 3. Workspace development web/dist
        let cwd = FileManager.default.currentDirectoryPath
        let webDist = URL(fileURLWithPath: cwd).appendingPathComponent("web/dist")
        if FileManager.default.fileExists(atPath: webDist.appendingPathComponent("index.html").path) {
            return webDist
        }
        
        return nil
    }
    
    private func startServer() {
        guard !isRunning else { return }
        
        guard let serverBinary = locateServerBinary() else {
            NSLog("StreamMenuBar: Unable to find stream-server binary")
            let alert = NSAlert()
            alert.messageText = "Server Binary Missing"
            alert.informativeText = "Could not find stream-server binary inside app bundle or workspace."
            alert.alertStyle = .critical
            alert.runModal()
            updateUIState(running: false)
            return
        }
        
        ensureDirectories()
        
        let dbPath = dataDirectoryURL.appendingPathComponent("data").path
        var args = ["--port", "\(port)", "--db-path", dbPath]
        
        if let webDir = locateWebDirectory() {
            args.append(contentsOf: ["--static-dir", webDir.path])
        }
        
        // Setup log file output
        if !FileManager.default.fileExists(atPath: logFileURL.path) {
            FileManager.default.createFile(atPath: logFileURL.path, contents: nil)
        }
        
        let process = Process()
        process.executableURL = serverBinary
        process.arguments = args
        
        if let handle = try? FileHandle(forWritingTo: logFileURL) {
            handle.seekToEndOfFile()
            self.logFileHandle = handle
            process.standardOutput = handle
            process.standardError = handle
        }
        
        process.terminationHandler = { [weak self] proc in
            NSLog("StreamMenuBar: stream-server process terminated with exit code \(proc.terminationStatus)")
            self?.updateUIState(running: false)
        }
        
        do {
            try process.run()
            self.serverProcess = process
            self.updateUIState(running: true)
            NSLog("StreamMenuBar: stream-server launched successfully (PID \(process.processIdentifier))")
        } catch {
            NSLog("StreamMenuBar: failed to run stream-server: \(error)")
            updateUIState(running: false)
        }
    }
    
    func stopServer(waitTimeout: TimeInterval = 2.0) {
        guard let process = serverProcess, process.isRunning else {
            updateUIState(running: false)
            return
        }
        
        let pid = process.processIdentifier
        NSLog("StreamMenuBar: stopping server PID \(pid)")
        
        kill(pid, SIGTERM)
        
        let deadline = Date().addingTimeInterval(waitTimeout)
        while process.isRunning && Date() < deadline {
            usleep(100_000) // 100ms
        }
        
        if process.isRunning {
            NSLog("StreamMenuBar: process did not exit in time, sending SIGKILL")
            kill(pid, SIGKILL)
        }
        
        self.serverProcess = nil
        try? logFileHandle?.close()
        self.logFileHandle = nil
        updateUIState(running: false)
    }
    
    // MARK: - Actions
    
    @objc private func openBrowserAction() {
        guard let url = URL(string: "http://127.0.0.1:\(port)") else { return }
        NSWorkspace.shared.open(url)
    }
    
    @objc private func copyServerAddressAction() {
        let addr = primaryServerAddress()
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(addr, forType: .string)
    }
    
    private func primaryServerAddress() -> String {
        var ifaddr: UnsafeMutablePointer<ifaddrs>?
        var lanIP: String?
        if getifaddrs(&ifaddr) == 0, let firstAddr = ifaddr {
            defer { freeifaddrs(firstAddr) }
            var ptr: UnsafeMutablePointer<ifaddrs>? = firstAddr
            while let current = ptr {
                let flags = Int32(current.pointee.ifa_flags)
                let isUp = (flags & IFF_UP) != 0
                let isLoopback = (flags & IFF_LOOPBACK) != 0
                if isUp && !isLoopback, let addr = current.pointee.ifa_addr, addr.pointee.sa_family == UInt8(AF_INET) {
                    var hostname = [CChar](repeating: 0, count: Int(NI_MAXHOST))
                    if getnameinfo(addr, socklen_t(addr.pointee.sa_len), &hostname, socklen_t(hostname.count), nil, 0, NI_NUMERICHOST) == 0 {
                        let ip = String(cString: hostname)
                        if ip.hasPrefix("192.168.") || ip.hasPrefix("10.") || ip.hasPrefix("172.") {
                            lanIP = ip
                            break
                        }
                    }
                }
                ptr = current.pointee.ifa_next
            }
        }
        if let lanIP {
            return "http://\(lanIP):\(port)"
        }
        return "http://127.0.0.1:\(port)"
    }
    
    private func startBonjourBroadcast() {
        stopBonjourBroadcast()
        let hostName = Host.current().localizedName ?? "Mello TV"
        let service = NetService(domain: "local.", type: "_mellotv._tcp.", name: hostName, port: Int32(port))
        let txtDict: [String: Data] = [
            "service": Data("mello-tv".utf8),
            "version": Data("api.v1".utf8),
            "path": Data("/api/v1/status".utf8)
        ]
        service.setTXTRecord(NetService.data(fromTXTRecord: txtDict))
        service.publish()
        self.bonjourService = service
    }
    
    private func stopBonjourBroadcast() {
        bonjourService?.stop()
        bonjourService = nil
    }
    
    @objc private func restartServerAction() {
        stopServer(waitTimeout: 2.0)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self] in
            self?.startServer()
        }
    }
    
    @objc private func toggleServerAction() {
        if isRunning {
            stopServer()
        } else {
            startServer()
        }
    }
    
    @objc private func openDataDirectoryAction() {
        NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: dataDirectoryURL.path)
    }
    
    @objc private func viewLogsAction() {
        if FileManager.default.fileExists(atPath: logFileURL.path) {
            NSWorkspace.shared.open(logFileURL)
        } else {
            NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: logsDirectoryURL.path)
        }
    }
    
    @objc private func quitAction() {
        stopServer(waitTimeout: 2.0)
        NSApplication.shared.terminate(nil)
    }
    
    // MARK: - Launch at Login (macOS LaunchAgent)
    
    private func isLaunchAtLoginEnabled() -> Bool {
        FileManager.default.fileExists(atPath: launchAgentPlistURL.path)
    }
    
    @objc private func toggleLaunchAtLoginAction() {
        if isLaunchAtLoginEnabled() {
            try? FileManager.default.removeItem(at: launchAgentPlistURL)
            launchAtLoginMenuItem.state = .off
        } else {
            guard let appPath = Bundle.main.bundleURL.path as String? else { return }
            let plistContent = """
            <?xml version="1.0" encoding="UTF-8"?>
            <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
            <plist version="1.0">
            <dict>
                <key>Label</key>
                <string>tv.stream.desktop</string>
                <key>ProgramArguments</key>
                <array>
                    <string>/usr/bin/open</string>
                    <string>\(appPath)</string>
                </array>
                <key>RunAtLoad</key>
                <true/>
                <key>ProcessType</key>
                <string>Interactive</string>
            </dict>
            </plist>
            """
            let launchAgentsDir = launchAgentPlistURL.deletingLastPathComponent()
            try? FileManager.default.createDirectory(at: launchAgentsDir, withIntermediateDirectories: true)
            try? plistContent.write(to: launchAgentPlistURL, atomically: true, encoding: .utf8)
            launchAtLoginMenuItem.state = .on
        }
    }
}

// MARK: - Main Application Entrypoint

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.accessory) // Ensures app runs purely as menu bar accessory without taking over focus

let sigtermSource = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
sigtermSource.setEventHandler {
    delegate.stopServer(waitTimeout: 2.0)
    exit(0)
}
sigtermSource.resume()
signal(SIGTERM, SIG_IGN)

let sigintSource = DispatchSource.makeSignalSource(signal: SIGINT, queue: .main)
sigintSource.setEventHandler {
    delegate.stopServer(waitTimeout: 2.0)
    exit(0)
}
sigintSource.resume()
signal(SIGINT, SIG_IGN)

app.run()
