import Foundation

struct ServerPaths {
    let dataDirectory: URL
    let logsDirectory: URL

    init() {
        let manager = FileManager.default
        let support = manager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let library = manager.urls(for: .libraryDirectory, in: .userDomainMask).first!
        dataDirectory = Self.migrateLegacyDirectory(in: support, legacyName: "Stream", name: "Mello TV")
        logsDirectory = Self.migrateLegacyDirectory(in: library.appendingPathComponent("Logs"), legacyName: "Stream", name: "Mello TV")
    }

    /// Move the complete, closed server directory together; never recreate or copy live databases.
    private static func migrateLegacyDirectory(in parent: URL, legacyName: String, name: String) -> URL {
        let manager = FileManager.default
        let current = parent.appendingPathComponent(name, isDirectory: true)
        let legacy = parent.appendingPathComponent(legacyName, isDirectory: true)
        guard !manager.fileExists(atPath: current.path), manager.fileExists(atPath: legacy.path) else { return current }
        do {
            try manager.moveItem(at: legacy, to: current)
            return current
        } catch {
            NSLog("Mello TV: keeping the existing server directory because renaming failed: %@", error.localizedDescription)
            return legacy
        }
    }
}
