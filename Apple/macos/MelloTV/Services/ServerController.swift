import Cocoa

@MainActor
final class ServerController {
    let port: Int
    let state = ServerProcessState()
    var onStateChange: ((Bool) -> Void)?
    private var serverProcess: Process?
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
    }

    private func setRunning(_ running: Bool) {
        state.isRunning = running
        if running { startBonjourBroadcast() } else { stopBonjourBroadcast() }
        onStateChange?(running)
    }

    func restart() {
        stop(waitTimeout: 2)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self] in self?.start() }
    }

    func toggle() { if isRunning { stop() } else { start() } }

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
        guard serverProcess?.isRunning != true else { return }
        
        guard let serverBinary = locateServerBinary() else {
            NSLog("Mello TV: Unable to find the Mello TV server binary")
            let alert = NSAlert()
            alert.messageText = "Server Binary Missing"
            alert.informativeText = "Could not find the Mello TV server inside the app bundle or workspace."
            alert.alertStyle = .critical
            alert.runModal()
            setRunning(false)
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
            _ = FileManager.default.createFile(atPath: logFileURL.path, contents: nil)
        }
        
        let process = Process()
        process.executableURL = serverBinary
        process.arguments = args
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
                self.setRunning(false)
            }
        }
        
        do {
            try process.run()
            self.serverProcess = process
            self.setRunning(true)
            NSLog("Mello TV: server process launched (PID \(process.processIdentifier))")
        } catch {
            NSLog("Mello TV: failed to run the server: \(error)")
            setRunning(false)
        }
    }
    
    func stop(waitTimeout: TimeInterval = 2.0) {
        guard let process = serverProcess, process.isRunning else {
            setRunning(false)
            return
        }
        
        let pid = process.processIdentifier
        NSLog("Mello TV: stopping server PID \(pid)")
        
        kill(pid, SIGTERM)
        
        let deadline = Date().addingTimeInterval(waitTimeout)
        while process.isRunning && Date() < deadline {
            usleep(100_000) // 100ms
        }
        
        if process.isRunning {
            NSLog("Mello TV: process did not exit in time, sending SIGKILL")
            kill(pid, SIGKILL)
        }
        
        self.serverProcess = nil
        try? logFileHandle?.close()
        self.logFileHandle = nil
        setRunning(false)
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
