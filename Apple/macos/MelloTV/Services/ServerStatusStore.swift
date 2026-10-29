import Foundation
import Darwin
import Combine
import MelloKit

@MainActor
final class ServerStatusStore: ObservableObject {
    @Published private(set) var snapshot: ServerSnapshot?
    @Published private(set) var isRefreshing = false
    @Published private(set) var error: String?
    @Published private(set) var lastRefreshed: Date?
    private let port: Int
    private var monitor: Task<Void, Never>?

    init(port: Int) { self.port = port }

    var isReachable: Bool { snapshot != nil && error == nil }

    func start() {
        guard monitor == nil else { return }
        monitor = Task { [weak self] in
            while !Task.isCancelled {
                guard let self = self else { return }
                await self.refresh()
                // A launched process may still be opening its HTTP listener.
                // Recover promptly instead of leaving the dashboard disconnected for 15 seconds.
                let interval: UInt64 = self.isReachable ? 15_000_000_000 : 1_000_000_000
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
        guard !isRefreshing else { return }
        isRefreshing = true
        defer { isRefreshing = false }
        do {
            // Process launch precedes listener readiness. Probe the loopback socket
            // before creating a URLSession task, which logs expected refusals as errors.
            let isListening = await LocalServerListener.isListening(port: port)
            guard !Task.isCancelled else { return }
            guard isListening else { throw URLError(.cannotConnectToHost) }
            let url = AppLinks.clientURL(port: port).appendingPathComponent("api/v1/status")
            let request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 6)
            let (data, response) = try await URLSession.shared.data(for: request)
            guard !Task.isCancelled else { return }
            guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw URLError(.badServerResponse) }
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            snapshot = try decoder.decode(ServerSnapshot.self, from: data)
            error = nil
            lastRefreshed = Date()
        } catch {
            guard !Task.isCancelled else { return }
            self.error = "Couldn't reach the local server."
        }
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
