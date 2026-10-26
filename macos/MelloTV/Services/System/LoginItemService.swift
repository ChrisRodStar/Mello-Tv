import Foundation

final class LoginItemService {
    private let fileManager = FileManager.default
    private var directory: URL {
        fileManager.urls(for: .libraryDirectory, in: .userDomainMask).first!
            .appendingPathComponent("LaunchAgents", isDirectory: true)
    }
    private var url: URL { directory.appendingPathComponent("com.mellowatch.desktop.plist") }
    private var legacyURL: URL { directory.appendingPathComponent("tv.stream.desktop.plist") }
    var isEnabled: Bool { fileManager.fileExists(atPath: url.path) || fileManager.fileExists(atPath: legacyURL.path) }

    func migrateExistingPreference() {
        if fileManager.fileExists(atPath: legacyURL.path) { setEnabled(true) }
    }

    func setEnabled(_ enabled: Bool) {
        do {
            if enabled {
                let content: [String: Any] = [
                    "Label": "com.mellowatch.desktop",
                    "ProgramArguments": ["/usr/bin/open", Bundle.main.bundleURL.path],
                    "RunAtLoad": true,
                    "ProcessType": "Interactive"
                ]
                try fileManager.createDirectory(at: directory, withIntermediateDirectories: true)
                let data = try PropertyListSerialization.data(fromPropertyList: content, format: .xml, options: 0)
                try data.write(to: url, options: .atomic)
            } else if fileManager.fileExists(atPath: url.path) {
                try fileManager.removeItem(at: url)
            }
            if fileManager.fileExists(atPath: legacyURL.path) { try fileManager.removeItem(at: legacyURL) }
        } catch { NSLog("Mello TV: couldn't update launch at login: %@", error.localizedDescription) }
    }
}
