import Foundation

enum ServerAddress {
    // Both device and simulator use the one deployed, private Tailscale origin.
    static let baseURL = URL(string: "http://127.0.0.1:8090")!
}
