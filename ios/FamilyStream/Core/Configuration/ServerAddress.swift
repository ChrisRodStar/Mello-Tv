import Foundation

enum ServerAddress {
    #if targetEnvironment(simulator)
    static let baseURL = URL(string: "http://127.0.0.1:8080")!
    static let fallbackURLs: [URL] = [
        URL(string: "http://10.0.0.12:8080")!,
        URL(string: "http://192.168.1.209:8080")!,
        URL(string: "http://100.99.24.1:8080")!,
    ]
    #else
    static let baseURL = URL(string: "http://10.0.0.12:8080")!
    static let fallbackURLs: [URL] = [
        URL(string: "http://192.168.1.209:8080")!,
        URL(string: "http://100.99.24.1:8080")!,
        URL(string: "http://Chriss-Mac-mini.local:8080")!,
        URL(string: "http://127.0.0.1:8080")!,
    ]
    #endif
}
