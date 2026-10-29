import Foundation
import Darwin
import Combine
import MelloKit

@MainActor
final class ServerStatusStore: ObservableObject {
    @Published private(set) var snapshot: ServerSnapshot?
    @Published private(set) var isRefreshing = false
    @Published private(set) var error: String?
    @Published private(set) var activeStreams: Int?
    @Published private(set) var userCount: Int?
    private let port: Int
    private var monitor: Task<Void, Never>?
    private var serverIsRunning: Bool
    private var startupBeganAt: Date?
    private var serverGeneration: UInt64 = 0

    init(port: Int, isServerRunning: Bool = false) {
        self.port = port
        serverIsRunning = isServerRunning
        startupBeganAt = isServerRunning ? Date() : nil
    }

    var hasConnected: Bool { snapshot != nil }
    var isStarting: Bool { serverIsRunning && !hasConnected && error == nil }
    var isReachable: Bool { serverIsRunning && hasConnected && error == nil }

    func serverStateChanged(isRunning: Bool) {
        serverGeneration &+= 1
        serverIsRunning = isRunning
        startupBeganAt = isRunning ? Date() : nil
        snapshot = nil
        error = nil
        activeStreams = nil
        userCount = nil
        if monitor != nil {
            stop()
            start()
        }
    }

    func start() {
        guard monitor == nil else { return }
        monitor = Task { [weak self] in
            while !Task.isCancelled {
                guard let self = self else { return }
                await self.refresh()
                // A launched process may still be opening its HTTP listener.
                // Recover promptly instead of leaving the dashboard disconnected for 15 seconds.
                let interval: UInt64 = self.isReachable || !self.serverIsRunning ? 15_000_000_000 : 1_000_000_000
                do { try await Task.sleep(nanoseconds: interval) }
                catch { return }
            }
        }
    }

    func stop() {
        monitor?.cancel()
        monitor = nil
    }

    func refreshNow() {
        Task { await refresh() }
    }

    private func refresh() async {
        guard serverIsRunning, !isRefreshing else { return }
        let generation = serverGeneration
        isRefreshing = true
        defer { isRefreshing = false }
        do {
            // Process launch precedes listener readiness. Probe the loopback socket
            // before creating a URLSession task, which logs expected refusals as errors.
            let isListening = await LocalServerListener.isListening(port: port)
            guard !Task.isCancelled, generation == serverGeneration else { return }
            guard isListening else { throw URLError(.cannotConnectToHost) }
            async let activity = fetchPlaybackActivity()
            async let profiles = fetchUserCount()
            let url = AppLinks.clientURL(port: port).appendingPathComponent("api/v1/status")
            let request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 6)
            let (data, response) = try await URLSession.shared.data(for: request)
            guard !Task.isCancelled, generation == serverGeneration else { return }
            guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw URLError(.badServerResponse) }
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            snapshot = try decoder.decode(ServerSnapshot.self, from: data)
            error = nil
            let (streamCount, profileCount) = await (activity, profiles)
            guard !Task.isCancelled, generation == serverGeneration else { return }
            activeStreams = streamCount
            userCount = profileCount
        } catch {
            guard !Task.isCancelled, generation == serverGeneration else { return }
            activeStreams = nil
            userCount = nil
            if hasConnected {
                self.error = "Lost connection to the local server."
            } else if let startupBeganAt, Date().timeIntervalSince(startupBeganAt) >= 30 {
                self.error = "The server is taking longer than expected to start. Open the logs for details."
            }
            // A refused connection before the first successful status response
            // is normal startup, not a lost connection.
        }
    }

    private func fetchUserCount() async -> Int? {
        let url = AppLinks.clientURL(port: port).appendingPathComponent("api/v1/profiles")
        let request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 3)
        do {
            let (data, response) = try await URLSession.shared.data(for: request)
            guard !Task.isCancelled, (response as? HTTPURLResponse)?.statusCode == 200 else { return nil }
            return try JSONDecoder().decode(ProfileCountReply.self, from: data).profiles.count
        } catch { return nil }
    }

    private func fetchPlaybackActivity() async -> Int? {
        let url = AppLinks.clientURL(port: port).appendingPathComponent("api/v1/update-readiness")
        let request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 3)
        do {
            let (data, response) = try await URLSession.shared.data(for: request)
            guard !Task.isCancelled, (response as? HTTPURLResponse)?.statusCode == 200 else { return nil }
            let activity = try JSONDecoder().decode(PlaybackActivity.self, from: data)
            return activity.activeStreams >= 0 ? activity.activeStreams : nil
        } catch {
            // Missing activity must not make an otherwise available server look offline.
            return nil
        }
    }
}

private struct ProfileCountReply: Decodable {
    struct Identity: Decodable { let id: String }
    let profiles: [Identity]
}

private struct PlaybackActivity: Decodable {
    let activeStreams: Int

    enum CodingKeys: String, CodingKey {
        case activeStreams = "active_streams"
    }
}

private enum LocalServerListener {
    static func isListening(port: Int) async -> Bool {
        await Task.detached(priority: .utility) {
            guard let port = UInt16(exactly: port), port != 0 else { return false }
            let descriptor = socket(AF_INET, SOCK_STREAM, 0)
            guard descriptor >= 0 else { return false }
            defer { close(descriptor) }
            guard fcntl(descriptor, F_SETFL, O_NONBLOCK) == 0 else { return false }

            var address = sockaddr_in()
            address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
            address.sin_family = sa_family_t(AF_INET)
            address.sin_port = port.bigEndian
            address.sin_addr = in_addr(s_addr: INADDR_LOOPBACK.bigEndian)
            let result = withUnsafePointer(to: &address) { pointer in
                pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    Darwin.connect(descriptor, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
                }
            }
            if result == 0 { return true }
            guard errno == EINPROGRESS else { return false }

            // Bound the probe and keep it off the main actor even if the listener stalls.
            var event = pollfd(fd: descriptor, events: Int16(POLLOUT), revents: 0)
            guard poll(&event, 1, 200) > 0 else { return false }
            var socketError: Int32 = 0
            var length = socklen_t(MemoryLayout<Int32>.size)
            return getsockopt(descriptor, SOL_SOCKET, SO_ERROR, &socketError, &length) == 0
                && socketError == 0
        }.value
    }
}
