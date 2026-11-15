import Foundation
import Network
import Combine

private struct ServerStatusReply: Decodable {
    let service: String?
    let serverName: String?
    let serverId: String?
    let status: String?
    let port: Int?
}

/// Manages the connection endpoint to the local or remote Mello TV server from Apple TV.
@MainActor
public final class ServerAddress: ObservableObject {
    public static let shared = ServerAddress()

    private static let serverURLKey = "mellotv.tvos.serverURL"
    private static let serverNameKey = "mellotv.tvos.serverName"

    public nonisolated static var currentBaseURL: URL? {
        if let storedString = UserDefaults.standard.string(forKey: serverURLKey),
           let url = URL(string: storedString) {
            return url
        }
        return nil
    }

    public nonisolated static var baseURL: URL {
        currentBaseURL ?? URL(string: "http://127.0.0.1:8080")!
    }

    public nonisolated static var fallbackURLs: [URL] {
        []
    }

    public nonisolated static var serverIdentityURL: URL {
        baseURL
    }

    public nonisolated static var tailscaleURL: URL? {
        nil
    }

    public nonisolated static var tailscaleCandidateURLs: [URL] {
        []
    }

    public nonisolated static var isCellularOnly: Bool {
        false
    }

    public nonisolated static var isTailscaleActive: Bool {
        false
    }

    public nonisolated static func isTailscaleAddress(_ url: URL) -> Bool {
        false
    }

    public nonisolated static func markEndpointActive(_ url: URL) {}

    public static func ensureResolved() async -> URL? {
        currentBaseURL
    }

    @Published public private(set) var activeBaseURL: URL?
    @Published public private(set) var serverName: String = "Mello TV Server"
    @Published public private(set) var isConnected: Bool = false
    @Published public private(set) var discoveredServers: [DiscoveredServerInfo] = []
    @Published public private(set) var isScanning: Bool = false

    private var browser: NWBrowser?

    public struct DiscoveredServerInfo: Identifiable, Hashable {
        public let id: String
        public let name: String
        public let url: URL
    }

    private init() {
        if let storedString = UserDefaults.standard.string(forKey: Self.serverURLKey),
           let url = URL(string: storedString) {
            self.activeBaseURL = url
            self.serverName = UserDefaults.standard.string(forKey: Self.serverNameKey) ?? "Mello TV Server"
            self.isConnected = true
        }
        startDiscovery()
    }

    public func connect(to url: URL, name: String = "Mello TV Server") async -> Bool {
        let trimmed = url.absoluteString.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard let validatedURL = URL(string: trimmed) else { return false }

        do {
            let statusURL = validatedURL.appendingPathComponent("api/v1/status")
            var request = URLRequest(url: statusURL)
            request.timeoutInterval = 4.0

            let (data, response) = try await URLSession.shared.data(for: request)
            guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
                return false
            }

            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            let status = try? decoder.decode(ServerStatusReply.self, from: data)
            let resolvedName = status?.serverName ?? name

            self.activeBaseURL = validatedURL
            self.serverName = resolvedName
            self.isConnected = true

            UserDefaults.standard.set(validatedURL.absoluteString, forKey: Self.serverURLKey)
            UserDefaults.standard.set(resolvedName, forKey: Self.serverNameKey)
            return true
        } catch {
            return false
        }
    }

    public func disconnect() {
        self.activeBaseURL = nil
        self.isConnected = false
        UserDefaults.standard.removeObject(forKey: Self.serverURLKey)
        UserDefaults.standard.removeObject(forKey: Self.serverNameKey)
    }

    public func startDiscovery() {
        guard !isScanning else { return }
        isScanning = true

        // 1. Simulator Loopback Probe:
        // On simulator, host Mac runs at 127.0.0.1:8080. Check it immediately like iOS does.
        #if targetEnvironment(simulator)
        Task {
            if let loopback = URL(string: "http://127.0.0.1:8080") {
                await probeAndAddServer(url: loopback, fallbackName: "Mac Server (Simulator)")
            }
        }
        #endif

        // 2. Bonjour discovery
        startBonjourDiscovery()
    }

    private func probeAndAddServer(url: URL, fallbackName: String) async {
        do {
            let statusURL = url.appendingPathComponent("api/v1/status")
            var request = URLRequest(url: statusURL)
            request.timeoutInterval = 2.0
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let http = response as? HTTPURLResponse, http.statusCode == 200 else { return }

            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            let status = try? decoder.decode(ServerStatusReply.self, from: data)
            guard status?.service == "mello-tv" || status?.status == "ok" else { return }

            let sName = status?.serverName ?? fallbackName
            let sId = status?.serverId ?? url.absoluteString
            let info = DiscoveredServerInfo(id: sId, name: sName, url: url)

            if !discoveredServers.contains(where: { $0.url == url || $0.id == info.id }) {
                discoveredServers.append(info)
            }

            // Auto-connect to the first discovered server if not connected yet (matching iOS zero-touch behavior)
            if !isConnected && activeBaseURL == nil {
                _ = await connect(to: url, name: sName)
            }
        } catch {
            // Server not reachable at this probe address
        }
    }

    public func startBonjourDiscovery() {
        let parameters = NWParameters()
        parameters.includePeerToPeer = true
        let browser = NWBrowser(for: .bonjour(type: "_mellotv._tcp", domain: "local."), using: parameters)

        browser.stateUpdateHandler = { [weak self] state in
            if case .failed = state {
                Task { @MainActor in
                    self?.isScanning = false
                }
            }
        }

        browser.browseResultsChangedHandler = { [weak self] results, _ in
            Task { @MainActor in
                for result in results {
                    if case let .service(name, _, _, _) = result.endpoint {
                        let defaultPort = 8080
                        if let url = URL(string: "http://\(name).local:\(defaultPort)") {
                            await self?.probeAndAddServer(url: url, fallbackName: name)
                        }
                    }
                }
            }
        }

        browser.start(queue: .main)
        self.browser = browser
    }

    public func stopDiscovery() {
        browser?.cancel()
        browser = nil
        isScanning = false
    }
}
