import Cocoa

@MainActor
final class ServerController {
    static let keepAwakeKey = "mellotv.native.keepAwake"
    let port: Int
    let state = ServerProcessState()
    var onStateChange: ((Bool) -> Void)?
    private var serverProcess: Process?
    private var intentionalStopProcess: Process?
    private var restartGeneration = 0
    private var consecutiveFailures = 0
    private var launchedAt: Date?
    private var awakeActivity: NSObjectProtocol?
    private var preferencesObserver: NSObjectProtocol?
    // The write end stays in the app. Even Xcode's force-stop closes it,
    // allowing the child to detect owner death without an app termination callback.
    private var serverLifetimePipe: Pipe?
    private var bonjourService: NetService?
    private var logFileHandle: FileHandle?
    var isRunning: Bool { state.isRunning }
    let dataDirectoryURL: URL
    let logsDirectoryURL: URL
    var logFileURL: URL { logsDirectoryURL.appendingPathComponent("MelloTV.log") }

    init(port: Int) {
        self.port = port
        let paths = ServerPaths()
        dataDirectoryURL = paths.dataDirectory
        logsDirectoryURL = paths.logsDirectory
        preferencesObserver = NotificationCenter.default.addObserver(forName: UserDefaults.didChangeNotification,
            object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor in self?.updateAwakeActivity() }
        }
    }

    private func setRunning(_ running: Bool) {
        state.isRunning = running
        updateAwakeActivity()
        if running { startBonjourBroadcast() } else { stopBonjourBroadcast() }
        onStateChange?(running)
    }

    func restart() {
        guard stopAndConfirm(waitTimeout: 2) else { return }
        let generation = restartGeneration
        state.isRestarting = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self] in
            guard let self, self.restartGeneration == generation else { return }
            self.start()
        }
    }

    func toggle() { if isRunning || state.isRestarting { stop() } else { start() } }

    func openDataDirectory() {
        NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: dataDirectoryURL.path)
    }

    func openLogs() {
        if FileManager.default.fileExists(atPath: logFileURL.path) { NSWorkspace.shared.open(logFileURL) }
        else { NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: logsDirectoryURL.path) }
    }

    func ensureDirectories() {
        try? FileManager.default.createDirectory(at: dataDirectoryURL.appendingPathComponent("data"), withIntermediateDirectories: true)
        try? FileManager.default.createDirectory(at: logsDirectoryURL, withIntermediateDirectories: true)
    }
    
    private func locateServerBinary() -> URL? {
        let bundleURL = Bundle.main.bundleURL
        
        // 1. Inside Mello TV.app/Contents/MacOS/MelloTVServer
        let inAppBundle = bundleURL.appendingPathComponent("Contents/MacOS/MelloTVServer")
        if FileManager.default.isExecutableFile(atPath: inAppBundle.path) {
            return inAppBundle
        }
        
        // 2. Next to executable
        if let exeDir = Bundle.main.executableURL?.deletingLastPathComponent() {
            let nextToExe = exeDir.appendingPathComponent("MelloTVServer")
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
        
        // 1. Inside Mello TV.app/Contents/Resources/web
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
    
    func start() {
        restartGeneration += 1
        consecutiveFailures = 0
        state.isRestarting = false
        startProcess()
    }

    private func startProcess() {
        guard serverProcess?.isRunning != true else { return }
        
        guard let serverBinary = locateServerBinary() else {
            NSLog("Mello TV: Unable to find the Mello TV server binary")
            let alert = NSAlert()
            alert.messageText = "Server Binary Missing"
            alert.informativeText = "Could not find the Mello TV server inside the app bundle or workspace."
            alert.alertStyle = .critical
            alert.runModal()
            state.message = "Server binary missing. Reinstall Mello TV."
            setRunning(false)
            return
        }
        
        ensureDirectories()
        
        let dbPath = dataDirectoryURL.appendingPathComponent("data").path
        var args = ["--port", "\(port)", "--db-path", dbPath, "--exit-on-stdin-close"]
        
        if let webDir = locateWebDirectory() {
            args.append(contentsOf: ["--static-dir", webDir.path])
        }
        
        // Setup log file output
        if !FileManager.default.fileExists(atPath: logFileURL.path) {
            _ = FileManager.default.createFile(atPath: logFileURL.path, contents: nil)
        }
        
        let process = Process()
        process.executableURL = serverBinary
        process.arguments = args
        let lifetimePipe = Pipe()
        process.standardInput = lifetimePipe
        var environment = ProcessInfo.processInfo.environment
        do {
            if let credential = try CatalogCredentialStore.read() {
                environment.removeValue(forKey: "TMDB_API_KEY")
                environment.removeValue(forKey: "TMDB_READ_ACCESS_TOKEN")
                environment.removeValue(forKey: "TMDB_ACCESS_TOKEN")
                let name = CatalogCredentialStore.isAPIKey(credential) ? "TMDB_API_KEY" : "TMDB_READ_ACCESS_TOKEN"
                environment[name] = credential
            }
        } catch {
            // Never log the credential or request URL.
            NSLog("Mello TV: personal catalog credential unavailable; using the server's default configuration.")
        }
        process.environment = environment
        
        if let handle = try? FileHandle(forWritingTo: logFileURL) {
            handle.seekToEndOfFile()
            self.logFileHandle = handle
            process.standardOutput = handle
            process.standardError = handle
        }
        
        process.terminationHandler = { [weak self] proc in
            NSLog("Mello TV: server process terminated with exit code \(proc.terminationStatus)")
            DispatchQueue.main.async {
                guard let self = self, self.serverProcess === proc else { return }
                self.serverProcess = nil
                self.closeServerLifetimePipe()
                try? self.logFileHandle?.close()
                self.logFileHandle = nil
                self.setRunning(false)
                if self.intentionalStopProcess === proc {
                    self.intentionalStopProcess = nil
                    return
                }
                if let launchedAt = self.launchedAt, Date().timeIntervalSince(launchedAt) >= 300 {
                    self.consecutiveFailures = 0
                }
                self.consecutiveFailures += 1
                let attempt = self.consecutiveFailures
                guard attempt <= 3 else {
                    self.state.message = "Server stopped after repeated unexpected exits. Open logs for details."
                    return
                }
                let delay = pow(2.0, Double(attempt - 1))
                self.state.isRestarting = true
                self.state.message = "Server stopped unexpectedly. Restarting (\(attempt) of 3)…"
                let generation = self.restartGeneration
                DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
                    guard let self, self.restartGeneration == generation else { return }
                    self.state.isRestarting = false
                    self.startProcess()
                }
            }
        }
        
        do {
            try process.run()
            self.serverProcess = process
            self.serverLifetimePipe = lifetimePipe
            self.launchedAt = Date()
            self.state.message = nil
            self.setRunning(true)
            NSLog("Mello TV: server process launched (PID \(process.processIdentifier))")
        } catch {
            try? lifetimePipe.fileHandleForWriting.close()
            try? lifetimePipe.fileHandleForReading.close()
            try? logFileHandle?.close()
            logFileHandle = nil
            NSLog("Mello TV: failed to run the server: \(error)")
            state.message = "Could not start the server. Open logs for details."
            setRunning(false)
        }
    }
    
    func stop(waitTimeout: TimeInterval = 2.0) {
        _ = stopAndConfirm(waitTimeout: waitTimeout)
    }

    /// Returns true only after the child has exited; callers restoring data must
    /// leave the data untouched if this returns false.
    @discardableResult
    func stopAndConfirm(waitTimeout: TimeInterval = 2.0) -> Bool {
        restartGeneration += 1
        consecutiveFailures = 0
        state.message = nil
        state.isRestarting = false
        guard let process = serverProcess, process.isRunning else {
            serverProcess = nil
            intentionalStopProcess = nil
            closeServerLifetimePipe()
            try? logFileHandle?.close()
            logFileHandle = nil
            setRunning(false)
            return true
        }
        intentionalStopProcess = process
        let pid = process.processIdentifier
        NSLog("Mello TV: stopping server PID \(pid)")
        kill(pid, SIGTERM)
        let deadline = Date().addingTimeInterval(max(0, waitTimeout))
        while process.isRunning && Date() < deadline { usleep(100_000) }
        if process.isRunning {
            NSLog("Mello TV: process did not exit in time, sending SIGKILL")
            kill(pid, SIGKILL)
            let killDeadline = Date().addingTimeInterval(2)
            while process.isRunning && Date() < killDeadline { usleep(100_000) }
        }
        guard !process.isRunning else {
            state.message = "The server could not be stopped. Try again before restoring data."
            return false
        }
        serverProcess = nil
        intentionalStopProcess = nil
        closeServerLifetimePipe()
        try? logFileHandle?.close()
        logFileHandle = nil
        setRunning(false)
        return true
    }

    private func updateAwakeActivity() {
        let shouldStayAwake = state.isRunning && UserDefaults.standard.bool(forKey: Self.keepAwakeKey)
        if shouldStayAwake && awakeActivity == nil {
            awakeActivity = ProcessInfo.processInfo.beginActivity(options: .idleSystemSleepDisabled,
                reason: "Keep the Mello TV server available")
        } else if !shouldStayAwake, let awakeActivity {
            ProcessInfo.processInfo.endActivity(awakeActivity)
            self.awakeActivity = nil
        }
    }

    private func closeServerLifetimePipe() {
        try? serverLifetimePipe?.fileHandleForWriting.close()
        serverLifetimePipe = nil
    }

    func primaryServerAddress() -> String {
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
    
    func startBonjourBroadcast() {
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
    
    func stopBonjourBroadcast() {
        bonjourService?.stop()
        bonjourService = nil
    }
    
}
