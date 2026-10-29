import Foundation

public enum ProfilePersistence {
    private static let keyPrefix = "family-stream.active-profile.v3"

    public static func activeProfileID(for serverURL: URL) -> String? {
        let key = storageKey(for: serverURL)
        return UserDefaults.standard.string(forKey: key)
    }

    public static func setActiveProfileID(_ id: String?, for serverURL: URL) {
        let key = storageKey(for: serverURL)
        if let id, !id.isEmpty {
            UserDefaults.standard.set(id, forKey: key)
        } else {
            UserDefaults.standard.removeObject(forKey: key)
        }
    }

    public static func storageKey(for serverURL: URL) -> String {
        let host = serverURL.host() ?? "default"
        let port = serverURL.port.map { ":\($0)" } ?? ""
        return "\(keyPrefix).\(host)\(port)"
    }
}
