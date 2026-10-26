import Foundation
import Combine

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
                do { try await Task.sleep(nanoseconds: 15_000_000_000) }
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
