import Foundation

public enum AppLinks {
    /// Official Mello Watch landing page.
    public static let websiteURL = URL(string: "https://mellowatch.com")!

    /// Official Mello Watch documentation page.
    public static let docsURL = URL(string: "https://mellowatch.com/docs")!

    /// Source code repository.
    public static let githubURL = URL(string: "https://github.com/ChrisRodStar/stream")!

    /// Alias for githubURL for backward compatibility.
    public static let sourceCodeURL = githubURL

    /// Creator donation / "Buy Me a Coffee" link.
    public static let buyMeACoffeeURL = URL(string: "https://buymeacoffee.com/chrissinclair")!

    /// Local HTTP client URL for a given port.
    public static func clientURL(port: Int) -> URL {
        var components = URLComponents()
        components.scheme = "http"
        components.host = "127.0.0.1"
        components.port = port
        components.path = "/"
        return components.url ?? URL(string: "http://127.0.0.1:\(port)/")!
    }
}

