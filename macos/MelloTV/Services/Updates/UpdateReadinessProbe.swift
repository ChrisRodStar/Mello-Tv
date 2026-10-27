import Foundation

struct UpdateReadinessProbe {
    let port: Int
    var session: URLSession = .shared

    func canRestartServer(isServerRunning: Bool) async -> Bool {
        guard isServerRunning else { return true }
        var components = URLComponents()
        components.scheme = "http"
        components.host = "127.0.0.1"
        components.port = port
        components.path = "/api/v1/update-readiness"
        guard let url = components.url else { return false }
        do {
            let request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 5)
            let (data, response) = try await session.data(for: request)
            guard !Task.isCancelled, (response as? HTTPURLResponse)?.statusCode == 200 else { return false }
            return try JSONDecoder().decode(UpdateReadiness.self, from: data).safeToRestart
        } catch {
            return false
        }
    }
}

private struct UpdateReadiness: Decodable {
    let safeToRestart: Bool

    enum CodingKeys: String, CodingKey {
        case safeToRestart = "safe_to_restart"
    }
}
