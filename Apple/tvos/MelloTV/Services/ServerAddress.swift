import Foundation
import Network
import Combine

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
        }
        startBonjourDiscovery()
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

            self.activeBaseURL = validatedURL
            self.serverName = name
            self.isConnected = true

            UserDefaults.standard.set(validatedURL.absoluteString, forKey: Self.serverURLKey)
            UserDefaults.standard.set(name, forKey: Self.serverNameKey)
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

    public func startBonjourDiscovery() {
        guard !isScanning else { return }
        isScanning = true

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
                var found: [DiscoveredServerInfo] = []
                for result in results {
                    if case let .service(name, _, _, _) = result.endpoint {
                        let defaultPort = 8080
                        if let url = URL(string: "http://\(name).local:\(defaultPort)") {
                            found.append(DiscoveredServerInfo(id: name, name: name, url: url))
                        }
                    }
                }
                self?.discoveredServers = found
            }
        }

        browser.start(queue: .main)
        self.browser = browser
    }

    public func stopBonjourDiscovery() {
        browser?.cancel()
        browser = nil
        isScanning = false
    }
}
