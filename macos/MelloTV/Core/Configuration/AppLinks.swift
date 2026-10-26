import Foundation

enum AppLinks {
    static let buyMeACoffeeURL = URL(string: "https://buymeacoffee.com/chrissinclair")!

    static func clientURL(port: Int) -> URL {
        var components = URLComponents()
        components.scheme = "http"
        components.host = "127.0.0.1"
        components.port = port
        components.path = "/"
        return components.url!
    }
}
