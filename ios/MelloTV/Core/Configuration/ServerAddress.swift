import Foundation
import Darwin

struct DiscoveredServer: Identifiable, Hashable, Sendable {
    let id: String
    let name: String
    let primaryURL: URL
    let endpoints: [URL]
}

private struct ServerStatusProbeReply: Decodable {
    let version: String
    let status: String?
    let serverId: String?
    let serverName: String?
    let port: Int?
    let endpoints: [String]?
}

private struct UDPDiscoveryPacket: Decodable {
    let service: String?
    let version: String?
    let serverId: String?
    let serverName: String?
    let port: Int?
    let endpoints: [String]?
}

enum ServerAddress {
    private static let activeURLKey = "mello-tv.server.active-url"
    private static let knownEndpointsKey = "mello-tv.server.known-endpoints"
    private static let serverNameKey = "mello-tv.server.name"
    private static let serverIDKey = "mello-tv.server.id"

    private static let defaultLoopback = URL(string: "http://127.0.0.1:8080")!

    /// Current preferred base URL (persisted across launches or resolved dynamically).
    static var baseURL: URL {
        if let raw = UserDefaults.standard.string(forKey: activeURLKey),
           let url = normalizeServerURL(raw) {
            return url
        }
        if let first = fallbackURLs.first {
            return first
        }
        return defaultLoopback
    }

    /// Human-readable name of the connected server, if known.
    static var serverName: String {
        UserDefaults.standard.string(forKey: serverNameKey) ?? (baseURL.host() ?? "Mello TV Server")
    }

    /// Stable household/server identifier used for profile persistence keys.
    static var serverIdentityURL: URL {
        if let serverID = UserDefaults.standard.string(forKey: serverIDKey), !serverID.isEmpty,
           let synthetic = URL(string: "http://\(serverID).mellotv.local:8080") {
            return synthetic
        }
        return baseURL
    }

    /// All remembered endpoints (LAN + Tailscale + mDNS) learned from previous connections.
    static var fallbackURLs: [URL] {
        var urls: [URL] = []
        if let saved = UserDefaults.standard.stringArray(forKey: knownEndpointsKey) {
            for raw in saved {
                if let url = normalizeServerURL(raw) {
                    urls.append(url)
                }
            }
        }
        #if targetEnvironment(simulator)
        urls.append(defaultLoopback)
        #endif
        var seen = Set<URL>()
        return urls.filter { seen.insert($0).inserted }
    }

    /// Normalizes a user-entered or discovered server address into a valid base `URL`.
    static func normalizeServerURL(_ input: String) -> URL? {
        var trimmed = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        if !trimmed.contains("://") {
            trimmed = "http://\(trimmed)"
        }
        while trimmed.hasSuffix("/") {
            trimmed.removeLast()
        }
        guard var components = URLComponents(string: trimmed),
              let scheme = components.scheme?.lowercased(),
              ["http", "https"].contains(scheme),
              let host = components.host, !host.isEmpty else {
            return nil
        }
        if components.port == nil && scheme == "http" && !host.hasSuffix(".ts.net") {
            components.port = 8080
        }
        components.path = ""
        components.query = nil
        components.fragment = nil
        return components.url
    }

    /// Saves a verified server and all its advertised LAN / Tailscale endpoints.
    static func saveDiscoveredServer(_ server: DiscoveredServer) {
        UserDefaults.standard.set(server.primaryURL.absoluteString, forKey: activeURLKey)
        UserDefaults.standard.set(server.name, forKey: serverNameKey)
        UserDefaults.standard.set(server.id, forKey: serverIDKey)

        let existing = UserDefaults.standard.stringArray(forKey: knownEndpointsKey) ?? []
        let incoming = ([server.primaryURL] + server.endpoints).map(\.absoluteString)
        var combined: [String] = []
        var seen = Set<String>()
        for item in incoming + existing {
            if seen.insert(item).inserted {
                combined.append(item)
            }
        }
        UserDefaults.standard.set(Array(combined.prefix(16)), forKey: knownEndpointsKey)
    }

    /// Updates the active origin when one of the known endpoints succeeds.
    static func markEndpointActive(_ url: URL) {
        UserDefaults.standard.set(url.absoluteString, forKey: activeURLKey)
        // Asynchronously refresh endpoint list (including Tailscale IP) from /api/v1/status
        Task.detached(priority: .utility) {
            if let server = await probeServerStatus(at: url, timeout: 3.0) {
                saveDiscoveredServer(server)
            }
        }
    }

    /// Verifies a manual address against `/api/v1/status` and saves it if valid.
    static func connectManually(to rawAddress: String) async throws -> DiscoveredServer {
        guard let candidate = normalizeServerURL(rawAddress) else {
            throw APIError.serverMessage("Enter a valid server address (for example 192.168.1.50:8080).")
        }
        guard let server = await probeServerStatus(at: candidate, timeout: 5.0) else {
            throw APIError.connection("Couldn’t connect to Mello TV at \(candidate.host() ?? rawAddress).")
        }
        saveDiscoveredServer(server)
        return server
    }

    /// Ensures a reachable server is selected. Tries saved endpoints first, then runs full LAN discovery.
    @discardableResult
    static func ensureResolved() async -> DiscoveredServer? {
        let savedCandidates = ([baseURL] + fallbackURLs).reduce(into: [URL]()) { acc, u in
            if !acc.contains(u) { acc.append(u) }
        }
        if !savedCandidates.isEmpty,
           let fastMatch = await raceStatusProbes(savedCandidates, timeout: 1.5) {
            saveDiscoveredServer(fastMatch)
            return fastMatch
        }

        let discovered = await discoverServers()
        if let first = discovered.first {
            saveDiscoveredServer(first)
            return first
        }
        return nil
    }

    /// Runs multi-protocol zero-config discovery on the user's current network:
    /// 1. UDP Broadcast on port 7359 (`MELLO_TV_DISCOVER`)
    /// 2. Apple Bonjour / mDNS (`_mellotv._tcp`)
    /// 3. Dynamic `/24` subnet scan derived from the iPhone's own active Wi-Fi interface
    static func discoverServers() async -> [DiscoveredServer] {
        async let udpResults = discoverViaUDPBroadcast(timeout: 0.9)
        async let bonjourURLs = discoverViaBonjour(timeout: 1.1)

        var found: [DiscoveredServer] = await udpResults
        let mDNS = await bonjourURLs
        if !mDNS.isEmpty {
            for url in mDNS {
                if let verified = await probeServerStatus(at: url, timeout: 1.5) {
                    found.append(verified)
                }
            }
        }

        // Deduplicate if UDP or Bonjour already succeeded
        found = deduplicateServers(found)
        if !found.isEmpty {
            return found
        }

        // Fallback: if the router blocks UDP broadcast & multicast mDNS, scan the device's own /24 subnet
        let subnetTargets = generateLocalSubnetCandidates(port: 8080)
        if let scanned = await scanSubnetCandidates(subnetTargets, timeout: 0.85) {
            found.append(scanned)
        }

        return deduplicateServers(found)
    }

    private static func deduplicateServers(_ servers: [DiscoveredServer]) -> [DiscoveredServer] {
        var seen = Set<String>()
        var result: [DiscoveredServer] = []
        for s in servers {
            if seen.insert(s.id).inserted {
                result.append(s)
            }
        }
        return result
    }

    // MARK: - Protocol 1: UDP Broadcast Discovery (Port 7359)

    private static func discoverViaUDPBroadcast(timeout: TimeInterval) async -> [DiscoveredServer] {
        await Task.detached(priority: .userInitiated) {
            let fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP)
            guard fd >= 0 else { return [DiscoveredServer]() }
            defer { close(fd) }

            var broadcastEnable: Int32 = 1
            setsockopt(fd, SOL_SOCKET, SO_BROADCAST, &broadcastEnable, socklen_t(MemoryLayout<Int32>.size))

            var reuseEnable: Int32 = 1
            setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &reuseEnable, socklen_t(MemoryLayout<Int32>.size))

            let wholeSeconds = Int(timeout)
            let microSeconds = Int32((timeout - Double(wholeSeconds)) * 1_000_000)
            var tv = timeval(tv_sec: wholeSeconds, tv_usec: microSeconds)
            setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, socklen_t(MemoryLayout<timeval>.size))

            let payload = Array("MELLO_TV_DISCOVER".utf8)
            let broadcastIPs = activeBroadcastAddresses() + ["255.255.255.255"]

            for bcast in Set(broadcastIPs) {
                var addr = sockaddr_in()
                addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
                addr.sin_family = sa_family_t(AF_INET)
                addr.sin_port = UInt16(7359).bigEndian
                guard inet_pton(AF_INET, bcast, &addr.sin_addr) == 1 else { continue }

                payload.withUnsafeBytes { rawBuf in
                    withUnsafePointer(to: &addr) { addrPtr in
                        addrPtr.withMemoryRebound(to: sockaddr.self, capacity: 1) { saPtr in
                            _ = sendto(fd, rawBuf.baseAddress, rawBuf.count, 0, saPtr, socklen_t(MemoryLayout<sockaddr_in>.size))
                        }
                    }
                }
            }

            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            var packets: [(UDPDiscoveryPacket, String)] = []
            let deadline = Date().addingTimeInterval(timeout)

            while Date() < deadline {
                var buffer = [UInt8](repeating: 0, count: 2048)
                var senderAddr = sockaddr_in()
                var senderLen = socklen_t(MemoryLayout<sockaddr_in>.size)

                let n = withUnsafeMutablePointer(to: &senderAddr) { addrPtr in
                    addrPtr.withMemoryRebound(to: sockaddr.self, capacity: 1) { saPtr in
                        recvfrom(fd, &buffer, buffer.count, 0, saPtr, &senderLen)
                    }
                }
                if n <= 0 { break }

                var senderHost = [CChar](repeating: 0, count: Int(NI_MAXHOST))
                let senderIP: String = withUnsafePointer(to: &senderAddr) { addrPtr in
                    addrPtr.withMemoryRebound(to: sockaddr.self, capacity: 1) { saPtr in
                        if getnameinfo(saPtr, senderLen, &senderHost, socklen_t(senderHost.count), nil, 0, NI_NUMERICHOST) == 0 {
                            return String(cString: senderHost)
                        }
                        return ""
                    }
                }

                let data = Data(buffer.prefix(n))
                if let packet = try? decoder.decode(UDPDiscoveryPacket.self, from: data),
                   packet.service == "mello-tv" || packet.version == "api.v1" {
                    packets.append((packet, senderIP))
                    break
                }
            }

            var verified: [DiscoveredServer] = []
            for (packet, senderIP) in packets {
                let port = packet.port ?? 8080
                var candidateURLs: [URL] = []
                if !senderIP.isEmpty, let directURL = URL(string: "http://\(senderIP):\(port)") {
                    candidateURLs.append(directURL)
                }
                for ep in packet.endpoints ?? [] {
                    if let u = normalizeServerURL(ep), !candidateURLs.contains(u) {
                        candidateURLs.append(u)
                    }
                }
                if let match = await raceStatusProbes(candidateURLs, timeout: 1.5) {
                    verified.append(match)
                }
            }
            return verified
        }.value
    }

    // MARK: - Protocol 2: Bonjour / mDNS (_mellotv._tcp)

    private static func discoverViaBonjour(timeout: TimeInterval) async -> [URL] {
        await MainActor.run {
            BonjourBrowserRunner.shared.browse(timeout: timeout)
        }.value
    }

    // MARK: - Protocol 3: Dynamic Active Interface Subnet Scanner

    private static func activeIPv4Interfaces() -> [(ip: String, netmask: String, broadcast: String?)] {
        var results: [(String, String, String?)] = []
        var ifaddr: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&ifaddr) == 0, let first = ifaddr else { return results }
        defer { freeifaddrs(first) }

        var ptr: UnsafeMutablePointer<ifaddrs>? = first
        while let current = ptr {
            let flags = Int32(current.pointee.ifa_flags)
            let isUp = (flags & IFF_UP) != 0
            let isLoopback = (flags & IFF_LOOPBACK) != 0
            if isUp && !isLoopback,
               let addr = current.pointee.ifa_addr,
               addr.pointee.sa_family == UInt8(AF_INET) {
                let ip = numericHost(from: addr)
                let mask = current.pointee.ifa_netmask.map { numericHost(from: $0) } ?? "255.255.255.0"
                let bcast: String? = if (flags & IFF_BROADCAST) != 0, let dst = current.pointee.ifa_dstaddr {
                    numericHost(from: dst)
                } else {
                    nil
                }
                if !ip.isEmpty && !ip.hasPrefix("169.254.") {
                    results.append((ip, mask, bcast))
                }
            }
            ptr = current.pointee.ifa_next
        }
        return results
    }

    private static func activeBroadcastAddresses() -> [String] {
        var list: [String] = []
        for iface in activeIPv4Interfaces() {
            if let bcast = iface.broadcast, !bcast.isEmpty {
                list.append(bcast)
            }
            let parts = iface.ip.split(separator: ".").compactMap { Int($0) }
            if parts.count == 4 && (parts[0] == 192 || parts[0] == 10 || parts[0] == 172) {
                list.append("\(parts[0]).\(parts[1]).\(parts[2]).255")
            }
        }
        return list
    }

    private static func generateLocalSubnetCandidates(port: Int) -> [URL] {
        var candidates: [URL] = []
        #if targetEnvironment(simulator)
        candidates.append(defaultLoopback)
        #endif

        for iface in activeIPv4Interfaces() {
            let parts = iface.ip.split(separator: ".").compactMap { Int($0) }
            guard parts.count == 4 else { continue }
            // Only scan private LAN subnets (skip Tailscale 100.64.0.0/10 /10 range)
            let isPrivateLAN = parts[0] == 192 || parts[0] == 10 || (parts[0] == 172 && (16...31).contains(parts[1]))
            guard isPrivateLAN else { continue }
            let prefix = "\(parts[0]).\(parts[1]).\(parts[2])"
            let selfHost = parts[3]
            // Order hosts closest to the device's own IP + common router/server IPs first
            let hosts = (1...254).filter { $0 != selfHost }.sorted { a, b in
                abs(a - selfHost) < abs(b - selfHost)
            }
            for h in hosts {
                if let u = URL(string: "http://\(prefix).\(h):\(port)") {
                    candidates.append(u)
                }
            }
        }
        return candidates
    }

    private static func scanSubnetCandidates(_ urls: [URL], timeout: TimeInterval) async -> DiscoveredServer? {
        let batchSize = 48
        var index = 0
        while index < urls.count {
            let end = min(index + batchSize, urls.count)
            let batch = Array(urls[index..<end])
            if let hit = await raceStatusProbes(batch, timeout: timeout) {
                return hit
            }
            index = end
        }
        return nil
    }

    private static func numericHost(from addr: UnsafePointer<sockaddr>) -> String {
        var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
        if getnameinfo(addr, socklen_t(addr.pointee.sa_len), &host, socklen_t(host.count), nil, 0, NI_NUMERICHOST) == 0 {
            return String(cString: host)
        }
        return ""
    }

    // MARK: - Status Endpoint Verification

    static func raceStatusProbes(_ urls: [URL], timeout: TimeInterval) async -> DiscoveredServer? {
        guard !urls.isEmpty else { return nil }
        return await withTaskGroup(of: DiscoveredServer?.self) { group in
            for url in urls {
                group.addTask {
                    await probeServerStatus(at: url, timeout: timeout)
                }
            }
            for await result in group {
                if let match = result {
                    group.cancelAll()
                    return match
                }
            }
            return nil
        }
    }

    static func probeServerStatus(at baseURL: URL, timeout: TimeInterval) async -> DiscoveredServer? {
        let statusURL = baseURL.appending(path: "api/v1/status")
        var request = URLRequest(url: statusURL)
        request.httpMethod = "GET"
        request.timeoutInterval = timeout
        request.setValue("application/json", forHTTPHeaderField: "Accept")

        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = timeout
        config.timeoutIntervalForResource = timeout
        config.waitsForConnectivity = false
        let session = URLSession(configuration: config)

        do {
            let (data, response) = try await session.data(for: request)
            guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
                return nil
            }
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            guard let reply = try? decoder.decode(ServerStatusProbeReply.self, from: data),
                  reply.version == "api.v1" else {
                return nil
            }

            let advertisedEndpoints = (reply.endpoints ?? []).compactMap(normalizeServerURL)
            var allEndpoints = [baseURL]
            for ep in advertisedEndpoints where !allEndpoints.contains(ep) {
                allEndpoints.append(ep)
            }

            let id = reply.serverId ?? (baseURL.host() ?? "mello-tv")
            let name = reply.serverName ?? (baseURL.host() ?? "Mello TV Server")
            return DiscoveredServer(
                id: id,
                name: name,
                primaryURL: baseURL,
                endpoints: allEndpoints
            )
        } catch {
            return nil
        }
    }
}

@MainActor
private final class BonjourBrowserRunner: NSObject, NetServiceBrowserDelegate, NetServiceDelegate {
    static let shared = BonjourBrowserRunner()
    private var browser: NetServiceBrowser?
    private var services: [NetService] = []
    private var discoveredURLs: [URL] = []

    func browse(timeout: TimeInterval) -> Task<[URL], Never> {
        Task { @MainActor in
            self.services.removeAll()
            self.discoveredURLs.removeAll()
            let netBrowser = NetServiceBrowser()
            netBrowser.delegate = self
            netBrowser.includesPeerToPeer = true
            self.browser = netBrowser
            netBrowser.searchForServices(ofType: "_mellotv._tcp.", inDomain: "local.")

            try? await Task.sleep(for: .milliseconds(Int(timeout * 1000)))
            netBrowser.stop()
            self.browser = nil
            return self.discoveredURLs
        }
    }

    nonisolated func netServiceBrowser(_ browser: NetServiceBrowser, didFind service: NetService, moreComing: Bool) {
        MainActor.assumeIsolated {
            services.append(service)
            service.delegate = self
            service.resolve(withTimeout: 1.0)
        }
    }

    nonisolated func netServiceDidResolveAddress(_ sender: NetService) {
        MainActor.assumeIsolated {
            let port = sender.port > 0 ? sender.port : 8080
            if let host = sender.hostName?.trimmingCharacters(in: CharacterSet(charactersIn: ".")), !host.isEmpty,
               let url = URL(string: "http://\(host):\(port)") {
                if !discoveredURLs.contains(url) {
                    discoveredURLs.append(url)
                }
            }
        }
    }
}
