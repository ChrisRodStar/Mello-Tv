import Foundation
import Combine
import Darwin
import CryptoKit
import CommonCrypto
import Security
import SQLite3

private enum BackupFailure: LocalizedError {
    case invalid(String)
    case rollbackIncomplete(String)
    var errorDescription: String? {
        switch self {
        case .invalid(let message), .rollbackIncomplete(let message): return message
        }
    }
}

private struct BackupEnvelope: Codable {
    let version: Int
    let rounds: Int
    let salt: Data
    let sealed: Data
}
private struct RestoreMarker: Codable {
    let rollbackDirectory: String
    let originals: [String]
}
private struct BackupPayload: Codable {
    let files: [String: Data]
    let catalogCredential: String?
}

/// All filesystem, SQLite and cryptographic work is serialized off the main actor.
actor HouseholdBackupService {
    private let dataRoot: URL
    private let fileManager = FileManager.default
    private let allowed = Set(["accounts.db", "catalog.db", "api-encryption"])
    private let rounds = 600_000
    private let maxBytes = 256 * 1024 * 1024
    init(dataRoot: URL) { self.dataRoot = dataRoot }

    private func privateDirectory(_ url: URL) throws {
        try fileManager.createDirectory(at: url, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try fileManager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
    }
    private func temporaryDirectory() throws -> URL {
        let url = fileManager.temporaryDirectory.appendingPathComponent("mellotv-backup-\(UUID().uuidString)", isDirectory: true)
        try privateDirectory(url)
        return url
    }
    private func key(passphrase: String, salt: Data, rounds: Int) throws -> SymmetricKey {
        guard !passphrase.isEmpty, passphrase.utf8.count <= 4096, salt.count == 16,
              (200_000...1_000_000).contains(rounds) else { throw BackupFailure.invalid("Invalid backup or passphrase.") }
        let password = Array(passphrase.utf8)
        var derived = [UInt8](repeating: 0, count: 32)
        let result = password.withUnsafeBytes { p in salt.withUnsafeBytes { s in
            CCKeyDerivationPBKDF(CCPBKDFAlgorithm(kCCPBKDF2), p.baseAddress?.assumingMemoryBound(to: Int8.self), password.count,
                                  s.baseAddress?.assumingMemoryBound(to: UInt8.self), salt.count,
                                  CCPseudoRandomAlgorithm(kCCPRFHmacAlgSHA256), UInt32(rounds), &derived, 32)
        }}
        guard result == kCCSuccess else { throw BackupFailure.invalid("Couldn’t derive the backup key.") }
        return SymmetricKey(data: derived)
    }
    private func integrity(_ path: URL) throws {
        var db: OpaquePointer?
        guard sqlite3_open_v2(path.path, &db, SQLITE_OPEN_READONLY, nil) == SQLITE_OK, let db else {
            if db != nil { sqlite3_close(db) }
            throw BackupFailure.invalid("Couldn’t open a backup database.")
        }
        defer { sqlite3_close(db) }
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, "PRAGMA integrity_check", -1, &statement, nil) == SQLITE_OK, let statement else {
            throw BackupFailure.invalid("Couldn’t check a backup database.")
        }
        defer { sqlite3_finalize(statement) }
        guard sqlite3_step(statement) == SQLITE_ROW, let value = sqlite3_column_text(statement, 0),
              String(cString: value) == "ok", sqlite3_step(statement) == SQLITE_DONE else {
            throw BackupFailure.invalid("A backup database failed its integrity check.")
        }
    }
    private func checkAccountsKey(_ path: URL, key: Data) throws {
        var db: OpaquePointer?
        guard sqlite3_open_v2(path.path, &db, SQLITE_OPEN_READONLY, nil) == SQLITE_OK, let db else {
            if db != nil { sqlite3_close(db) }
            throw BackupFailure.invalid("Couldn’t inspect the accounts database.")
        }
        defer { sqlite3_close(db) }
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, "SELECT value FROM settings WHERE key='encryption-check'", -1, &statement, nil) == SQLITE_OK,
              let statement else { throw BackupFailure.invalid("The accounts database is not a supported Mello TV database.") }
        defer { sqlite3_finalize(statement) }
        guard sqlite3_step(statement) == SQLITE_ROW, let bytes = sqlite3_column_blob(statement, 0) else {
            throw BackupFailure.invalid("The accounts database encryption check is missing.")
        }
        let envelope = Data(bytes: bytes, count: Int(sqlite3_column_bytes(statement, 0)))
        do {
            let box = try AES.GCM.SealedBox(combined: envelope)
            let plain = try AES.GCM.open(box, using: SymmetricKey(data: key), authenticating: Data("api-key-check".utf8))
            guard plain == Data("family-stream-api.v1".utf8) else { throw BackupFailure.invalid("The accounts encryption key does not match.") }
        } catch { throw BackupFailure.invalid("The accounts encryption key does not match the database.") }
    }
    private func snapshot(_ source: URL, to destination: URL) throws {
        var src: OpaquePointer?, dst: OpaquePointer?
        guard sqlite3_open_v2(source.path, &src, SQLITE_OPEN_READONLY, nil) == SQLITE_OK, let src else {
            if src != nil { sqlite3_close(src) }
            throw BackupFailure.invalid("Couldn’t open \(source.lastPathComponent) for backup.")
        }
        defer { sqlite3_close(src) }
        guard sqlite3_open_v2(destination.path, &dst, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE, nil) == SQLITE_OK, let dst else {
            if dst != nil { sqlite3_close(dst) }
            throw BackupFailure.invalid("Couldn’t create a database snapshot.")
        }
        defer { sqlite3_close(dst) }
        guard let backup = sqlite3_backup_init(dst, "main", src, "main") else { throw BackupFailure.invalid("Couldn’t start a database snapshot.") }
        var result: Int32 = SQLITE_OK
        var busyCount = 0
        let deadline = Date().addingTimeInterval(30)
        repeat {
            guard Date() < deadline else { break }
            result = sqlite3_backup_step(backup, 128)
            if result == SQLITE_BUSY || result == SQLITE_LOCKED {
                busyCount += 1
                if busyCount > 100 { break }
                usleep(50_000)
            } else if result == SQLITE_OK { busyCount = 0 }
        } while result == SQLITE_OK || result == SQLITE_BUSY || result == SQLITE_LOCKED
        let finished = sqlite3_backup_finish(backup)
        guard result == SQLITE_DONE, finished == SQLITE_OK else { throw BackupFailure.invalid("Database is busy. Try the backup again.") }
        try fileManager.setAttributes([.posixPermissions: 0o600], ofItemAtPath: destination.path)
        try integrity(destination)
    }
    private func existsWithoutFollowingLinks(_ url: URL) throws -> Bool {
        var info = stat()
        if lstat(url.path, &info) == 0 {
            guard info.st_mode & S_IFMT == S_IFREG else { throw BackupFailure.invalid("A server data file is not a regular file: \(url.lastPathComponent).") }
            return true
        }
        guard errno == ENOENT else { throw BackupFailure.invalid("Couldn’t inspect \(url.lastPathComponent).") }
        return false
    }
    private func requireDirectory(_ url: URL) throws {
        var info = stat()
        guard lstat(url.path, &info) == 0, info.st_mode & S_IFMT == S_IFDIR else {
            throw BackupFailure.rollbackIncomplete("The rollback directory is unavailable or unsafe. The server remains stopped.")
        }
    }
    private func requireRegularFile(_ url: URL) throws {
        var info = stat()
        guard lstat(url.path, &info) == 0, info.st_mode & S_IFMT == S_IFREG else {
            throw BackupFailure.invalid("A server data file is not a regular file: \(url.lastPathComponent).")
        }
    }
    // Every server process must hold a shared lock for its entire lifetime.
    private func acquireRestoreLock() throws -> Int32 {
        try privateDirectory(dataRoot)
        let descriptor = open(dataRoot.appendingPathComponent(".household.lock").path, O_RDWR | O_CREAT | O_NOFOLLOW, mode_t(0o600))
        guard descriptor >= 0 else { throw BackupFailure.invalid("Couldn’t open the household lock.") }
        guard flock(descriptor, LOCK_EX | LOCK_NB) == 0 else {
            close(descriptor)
            throw BackupFailure.invalid("Another server is using this household. Stop it before restoring.")
        }
        return descriptor
    }
    private func syncDirectory(_ directory: URL) throws {
        let descriptor = open(directory.path, O_RDONLY)
        guard descriptor >= 0 else { throw BackupFailure.invalid("Couldn’t open the backup directory for synchronization.") }
        defer { close(descriptor) }
        guard fsync(descriptor) == 0 else { throw BackupFailure.invalid("Couldn’t synchronize the backup directory.") }
    }
    private func syncFile(_ url: URL) throws {
        let descriptor = open(url.path, O_RDONLY)
        guard descriptor >= 0 else { throw BackupFailure.invalid("Couldn’t open a rollback file.") }
        defer { close(descriptor) }
        guard fsync(descriptor) == 0 else { throw BackupFailure.invalid("Couldn’t synchronize a rollback file.") }
    }
    private func writePrivate(_ data: Data, to url: URL) throws {
        guard data.count <= maxBytes * 2 else { throw BackupFailure.invalid("Backup exceeds the supported size.") }
        let temporary = url.deletingLastPathComponent().appendingPathComponent(".mellotv-\(UUID().uuidString)")
        defer { try? fileManager.removeItem(at: temporary) }
        guard fileManager.createFile(atPath: temporary.path, contents: nil, attributes: [.posixPermissions: 0o600]) else { throw BackupFailure.invalid("Couldn’t create the backup file.") }
        let handle = try FileHandle(forWritingTo: temporary)
        try handle.write(contentsOf: data)
        try handle.synchronize()
        try handle.close()
        if rename(temporary.path, url.path) != 0 { throw BackupFailure.invalid("Couldn’t save the backup file.") }
        try syncDirectory(url.deletingLastPathComponent())
    }
    func create(to destination: URL, passphrase: String, catalogCredential: String?) throws {
        let destinationPath = destination.resolvingSymlinksInPath().standardizedFileURL.path
        let protectedPath = dataRoot.resolvingSymlinksInPath().standardizedFileURL.path + "/"
        guard !destinationPath.hasPrefix(protectedPath) && destinationPath != dataRoot.resolvingSymlinksInPath().standardizedFileURL.path else {
            throw BackupFailure.invalid("Choose a backup location outside the server data directory.")
        }
        let scratch = try temporaryDirectory()
        defer { try? fileManager.removeItem(at: scratch) }
        var files: [String: Data] = [:]
        for name in ["accounts.db", "catalog.db"] {
            let snap = scratch.appendingPathComponent(name)
            let source = dataRoot.appendingPathComponent(name)
            try requireRegularFile(source)
            try snapshot(source, to: snap)
            let size = try fileManager.attributesOfItem(atPath: snap.path)[.size] as? NSNumber
            guard let size, size.intValue <= maxBytes else { throw BackupFailure.invalid("Database exceeds the supported backup size.") }
            files[name] = try Data(contentsOf: snap, options: .mappedIfSafe)
        }
        let keyURL = dataRoot.appendingPathComponent("api-encryption")
        var existing: Data?
        if try existsWithoutFollowingLinks(keyURL) {
            let size = try fileManager.attributesOfItem(atPath: keyURL.path)[.size] as? NSNumber
            if size?.intValue == 32 { existing = try Data(contentsOf: keyURL) }
        }
        // Match the server fallback, including a malformed existing key file.
        files["api-encryption"] = existing ?? Data(repeating: 0x5c, count: 32)
        try checkAccountsKey(scratch.appendingPathComponent("accounts.db"), key: files["api-encryption"]!)
        guard files.values.reduce(0, { $0 + $1.count }) <= maxBytes else { throw BackupFailure.invalid("Backup exceeds the supported size.") }
        let payload = try JSONEncoder().encode(BackupPayload(files: files, catalogCredential: catalogCredential))
        var salt = Data(count: 16)
        let random = salt.withUnsafeMutableBytes { SecRandomCopyBytes(kSecRandomDefault, 16, $0.baseAddress!) }
        guard random == errSecSuccess else { throw BackupFailure.invalid("Couldn’t generate a backup salt.") }
        let sealed = try AES.GCM.seal(payload, using: key(passphrase: passphrase, salt: salt, rounds: rounds))
        guard let combined = sealed.combined else { throw BackupFailure.invalid("Couldn’t encrypt the backup.") }
        try writePrivate(try JSONEncoder().encode(BackupEnvelope(version: 1, rounds: rounds, salt: salt, sealed: combined)), to: destination)
    }
    private func validateWithServer(_ directory: URL) throws {
        var candidates = [Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/MelloTVServer")]
        if let executable = Bundle.main.executableURL { candidates.append(executable.deletingLastPathComponent().appendingPathComponent("MelloTVServer")) }
        guard let executable = candidates.first(where: { fileManager.isExecutableFile(atPath: $0.path) }) else {
            throw BackupFailure.invalid("The server validator is unavailable. Reinstall Mello TV before restoring.")
        }
        let process = Process()
        process.executableURL = executable
        process.arguments = ["--validate-data", "--db-path", directory.path]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        let deadline = Date().addingTimeInterval(30)
        while process.isRunning && Date() < deadline { usleep(50_000) }
        if process.isRunning {
            process.terminate()
            let stopDeadline = Date().addingTimeInterval(2)
            while process.isRunning && Date() < stopDeadline { usleep(50_000) }
            if process.isRunning { kill(process.processIdentifier, SIGKILL) }
            process.waitUntilExit()
            throw BackupFailure.invalid("The server validator timed out.")
        }
        guard process.terminationReason == .exit, process.terminationStatus == 0 else {
            throw BackupFailure.invalid("This backup isn’t compatible with the installed server.")
        }
    }
    /// Validate all content and both databases before the caller stops the server.
    func validate(file: URL, passphrase: String) throws -> ValidatedHouseholdBackup {
        let size = try fileManager.attributesOfItem(atPath: file.path)[.size] as? NSNumber
        guard let size, size.intValue > 0, size.intValue <= maxBytes * 2 else { throw BackupFailure.invalid("Backup file is too large or empty.") }
        let envelope = try JSONDecoder().decode(BackupEnvelope.self, from: Data(contentsOf: file))
        guard envelope.version == 1, envelope.salt.count == 16, envelope.sealed.count <= maxBytes * 2,
              (200_000...1_000_000).contains(envelope.rounds) else { throw BackupFailure.invalid("Unsupported or invalid backup.") }
        let payloadData: Data
        do { payloadData = try AES.GCM.open(AES.GCM.SealedBox(combined: envelope.sealed), using: key(passphrase: passphrase, salt: envelope.salt, rounds: envelope.rounds)) }
        catch { throw BackupFailure.invalid("The backup is damaged or the passphrase is incorrect.") }
        let payload = try JSONDecoder().decode(BackupPayload.self, from: payloadData)
        guard Set(payload.files.keys) == allowed, payload.files["api-encryption"]?.count == 32,
              payload.files.values.reduce(0, { $0 + $1.count }) <= maxBytes,
              (payload.catalogCredential?.utf8.count ?? 0) <= 4096,
              payload.catalogCredential.map({ !$0.isEmpty && $0.utf8.allSatisfy { $0 > 32 && $0 < 127 } }) ?? true else { throw BackupFailure.invalid("Backup contents are invalid.") }
        let scratch = try temporaryDirectory()
        defer { try? fileManager.removeItem(at: scratch) }
        for name in ["accounts.db", "catalog.db"] {
            guard let data = payload.files[name] else { throw BackupFailure.invalid("A database is missing.") }
            let url = scratch.appendingPathComponent(name)
            try writePrivate(data, to: url)
            try integrity(url)
        }
        try checkAccountsKey(scratch.appendingPathComponent("accounts.db"), key: payload.files["api-encryption"]!)
        try writePrivate(payload.files["api-encryption"]!, to: scratch.appendingPathComponent("api-encryption"))
        try validateWithServer(scratch)
        return ValidatedHouseholdBackup(files: payload.files, catalogCredential: payload.catalogCredential)
    }
    private let restoreNames = ["accounts.db", "accounts.db-wal", "accounts.db-shm", "catalog.db", "catalog.db-wal", "catalog.db-shm", "api-encryption"]
    private var markerURL: URL { dataRoot.appendingPathComponent(".restore-in-progress") }

    /// Recover before the server is allowed to open either database.
    func recoverInterruptedRestore() throws -> Bool {
        guard try existsWithoutFollowingLinks(markerURL) else { return false }
        let lock = try acquireRestoreLock()
        defer { flock(lock, LOCK_UN); close(lock) }
        return try recoverInterruptedRestoreUnlocked()
    }
    private func recoverInterruptedRestoreUnlocked() throws -> Bool {
        guard try existsWithoutFollowingLinks(markerURL) else { return false }
        try requireRegularFile(markerURL)
        let markerSize = try fileManager.attributesOfItem(atPath: markerURL.path)[.size] as? NSNumber
        guard let markerSize, markerSize.intValue > 0, markerSize.intValue <= 4096 else {
            throw BackupFailure.rollbackIncomplete("The restore recovery marker is invalid. The server remains stopped.")
        }
        let marker = try JSONDecoder().decode(RestoreMarker.self, from: Data(contentsOf: markerURL))
        let prefix = ".restore-rollback-"
        guard marker.rollbackDirectory.hasPrefix(prefix),
              UUID(uuidString: String(marker.rollbackDirectory.dropFirst(prefix.count))) != nil,
              marker.rollbackDirectory == URL(fileURLWithPath: marker.rollbackDirectory).lastPathComponent,
              Set(marker.originals).count == marker.originals.count,
              Set(marker.originals).isSubset(of: Set(restoreNames)) else {
            throw BackupFailure.rollbackIncomplete("The restore recovery marker is invalid. The server remains stopped.")
        }
        let rollback = dataRoot.appendingPathComponent(marker.rollbackDirectory, isDirectory: true)
        try requireDirectory(rollback)
        for name in restoreNames {
            let current = dataRoot.appendingPathComponent(name)
            let original = rollback.appendingPathComponent(name)
            if marker.originals.contains(name) {
                guard try existsWithoutFollowingLinks(original) else {
                    throw BackupFailure.rollbackIncomplete("Original files are missing from \(rollback.path). The server remains stopped.")
                }
                try requireRegularFile(original)
                let staged = dataRoot.appendingPathComponent(".recovery-\(UUID().uuidString)")
                defer { try? fileManager.removeItem(at: staged) }
                try fileManager.copyItem(at: original, to: staged)
                try syncFile(staged)
                guard rename(staged.path, current.path) == 0 else {
                    throw BackupFailure.rollbackIncomplete("Couldn’t restore original files from \(rollback.path). The server remains stopped.")
                }
                try syncDirectory(dataRoot)
            } else if try existsWithoutFollowingLinks(current) {
                try fileManager.removeItem(at: current)
            }
        }
        try syncDirectory(dataRoot)
        try fileManager.removeItem(at: markerURL)
        try syncDirectory(dataRoot)
        try? fileManager.removeItem(at: rollback)
        return true
    }

    /// Caller must first confirm the server child has exited. The marker and
    /// private rollback directory allow recovery if the process stops mid-swap.
    func restore(_ backup: ValidatedHouseholdBackup) throws {
        let lock = try acquireRestoreLock()
        defer { flock(lock, LOCK_UN); close(lock) }
        guard try !existsWithoutFollowingLinks(markerURL) else {
            throw BackupFailure.rollbackIncomplete("An earlier restore needs recovery. The server remains stopped.")
        }
        let rollback = dataRoot.appendingPathComponent(".restore-rollback-\(UUID().uuidString)", isDirectory: true)
        try privateDirectory(rollback)
        var originals: [String] = []
        for name in restoreNames where try existsWithoutFollowingLinks(dataRoot.appendingPathComponent(name)) { originals.append(name) }
        let marker = RestoreMarker(rollbackDirectory: rollback.lastPathComponent, originals: originals)
        do {
            // Complete and synchronize rollback copies and replacements before
            // publishing the marker or changing the live directory.
            for name in originals {
                let source = dataRoot.appendingPathComponent(name)
                try requireRegularFile(source)
                let original = rollback.appendingPathComponent(name)
                try fileManager.copyItem(at: source, to: original)
                try fileManager.setAttributes([.posixPermissions: 0o600], ofItemAtPath: original.path)
                try syncFile(original)
            }
            for name in ["accounts.db", "catalog.db", "api-encryption"] {
                guard let data = backup.files[name] else { throw BackupFailure.invalid("Backup is incomplete.") }
                try writePrivate(data, to: rollback.appendingPathComponent("new-" + name))
            }
            try syncDirectory(rollback)
            try syncDirectory(dataRoot)
            try writePrivate(try JSONEncoder().encode(marker), to: markerURL)
            for name in ["accounts.db-wal", "accounts.db-shm", "catalog.db-wal", "catalog.db-shm"] {
                let sidecar = dataRoot.appendingPathComponent(name)
                if try existsWithoutFollowingLinks(sidecar) { try fileManager.removeItem(at: sidecar) }
            }
            try syncDirectory(dataRoot)
            for name in ["accounts.db", "catalog.db", "api-encryption"] {
                let staged = rollback.appendingPathComponent("new-" + name)
                let target = dataRoot.appendingPathComponent(name)
                guard rename(staged.path, target.path) == 0 else { throw BackupFailure.invalid("Couldn’t install a restored database.") }
                try syncDirectory(dataRoot)
                try syncDirectory(rollback)
            }
            try integrity(dataRoot.appendingPathComponent("accounts.db"))
            try integrity(dataRoot.appendingPathComponent("catalog.db"))
            try fileManager.removeItem(at: markerURL)
            do { try syncDirectory(dataRoot) }
            catch { throw BackupFailure.rollbackIncomplete("Restore completed, but its commit could not be synchronized. The server remains stopped; the previous files are in \(rollback.path).") }
            // Retain a private rollback copy after a successful restore.
        } catch {
            if case .some(.rollbackIncomplete(_)) = error as? BackupFailure,
               !fileManager.fileExists(atPath: markerURL.path) { throw error }
            do {
                if fileManager.fileExists(atPath: markerURL.path) { _ = try recoverInterruptedRestoreUnlocked() }
                else { try? fileManager.removeItem(at: rollback) }
            } catch {
                throw BackupFailure.rollbackIncomplete("Restore failed and automatic rollback was incomplete. Original files remain in \(rollback.path). Do not restart the server.")
            }
            throw error
        }
    }
    func removeOwnedBackups(_ names: [String], in directory: URL) throws {
        for name in names {
            guard name == URL(fileURLWithPath: name).lastPathComponent,
                  name.hasPrefix("mellotv-auto-"), name.hasSuffix(".mellobackup") else { continue }
            let url = directory.appendingPathComponent(name)
            if fileManager.fileExists(atPath: url.path) { try fileManager.removeItem(at: url) }
        }
    }
    func prepareAutomaticDirectory(_ url: URL) throws { try privateDirectory(url) }
}

struct ValidatedHouseholdBackup: Sendable {
    fileprivate let files: [String: Data]
    let catalogCredential: String?
}

private enum BackupPassphraseStore {
    private static let query: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: "com.tinymellow.mellotv.backup",
        kSecAttrAccount as String: "automatic-passphrase",
        kSecAttrSynchronizable as String: false
    ]
    static func read() throws -> String? {
        var lookup = query
        lookup[kSecReturnData as String] = true
        lookup[kSecMatchLimit as String] = kSecMatchLimitOne
        var value: CFTypeRef?
        let status = SecItemCopyMatching(lookup as CFDictionary, &value)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = value as? Data, let text = String(data: data, encoding: .utf8) else {
            throw BackupFailure.invalid("Couldn’t read the automatic backup passphrase from Keychain.")
        }
        return text
    }
    static func save(_ passphrase: String) throws {
        let attributes: [String: Any] = [kSecValueData as String: Data(passphrase.utf8)]
        let status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            var item = query.merging(attributes) { _, new in new }
            item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            guard SecItemAdd(item as CFDictionary, nil) == errSecSuccess else { throw BackupFailure.invalid("Couldn’t save the passphrase in Keychain.") }
        } else if status != errSecSuccess { throw BackupFailure.invalid("Couldn’t save the passphrase in Keychain.") }
    }
    static func remove() throws {
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw BackupFailure.invalid("Couldn’t remove the automatic backup passphrase.") }
    }
}

@MainActor
final class HouseholdBackupCoordinator: ObservableObject {
    @Published private(set) var busy = false
    @Published private(set) var message: String?
    @Published private(set) var automaticEnabled = UserDefaults.standard.bool(forKey: "mellotv.backup.automatic")
    private let service: HouseholdBackupService
    private let automaticDirectory: URL
    private let isServerRunning: @MainActor () -> Bool
    private let stopServer: @MainActor () async -> Bool
    private let startServer: @MainActor () -> Void
    private var schedule: Task<Void, Never>?
    private let lastBackupKey = "mellotv.backup.lastAutomatic"
    private let includeCredentialKey = "mellotv.backup.includeCatalogCredential"
    private let ownedBackupsKey = "mellotv.backup.ownedFiles"

    init(dataDirectoryURL: URL, isServerRunning: @escaping @MainActor () -> Bool,
         stopServer: @escaping @MainActor () async -> Bool, startServer: @escaping @MainActor () -> Void) {
        service = HouseholdBackupService(dataRoot: dataDirectoryURL.appendingPathComponent("data", isDirectory: true))
        automaticDirectory = dataDirectoryURL.appendingPathComponent("Backups", isDirectory: true)
        self.isServerRunning = isServerRunning
        self.stopServer = stopServer
        self.startServer = startServer
    }
    /// Must complete successfully before starting the server after application launch.
    func recoverInterruptedRestore() async -> Bool {
        do {
            if try await service.recoverInterruptedRestore() {
                message = "An interrupted restore was rolled back to the previous household."
            }
            return true
        } catch {
            message = "Couldn’t recover an interrupted restore. The server remains stopped: \(error.localizedDescription)"
            return false
        }
    }
    func startAutomaticSchedule() {
        guard schedule == nil else { return }
        schedule = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                await self.performAutomaticIfDue()
                do { try await Task.sleep(nanoseconds: 3_600_000_000_000) } catch { return }
            }
        }
    }
    func enableAutomatic(passphrase: String, includeCredential: Bool) throws {
        guard passphrase.count >= 12, passphrase.utf8.count <= 4096 else { throw BackupFailure.invalid("Choose a backup passphrase of at least 12 characters.") }
        try BackupPassphraseStore.save(passphrase)
        UserDefaults.standard.set(includeCredential, forKey: includeCredentialKey)
        UserDefaults.standard.set(true, forKey: "mellotv.backup.automatic")
        automaticEnabled = true
        message = "Daily backups are enabled. Keep the passphrase somewhere safe."
        Task { await performAutomaticIfDue() }
    }
    func disableAutomatic() throws {
        try BackupPassphraseStore.remove()
        UserDefaults.standard.set(false, forKey: "mellotv.backup.automatic")
        automaticEnabled = false
        message = "Daily backups are off. Existing backup files remain available."
    }
    func create(to url: URL, passphrase: String, includeCredential: Bool) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        do {
            let credential = includeCredential ? try CatalogCredentialStore.read() : nil
            try await service.create(to: url, passphrase: passphrase, catalogCredential: credential)
            message = "Encrypted backup saved."
        } catch { message = error.localizedDescription }
    }
    private func performAutomaticIfDue() async {
        guard automaticEnabled, !busy else { return }
        let last = UserDefaults.standard.object(forKey: lastBackupKey) as? Date ?? .distantPast
        guard Date().timeIntervalSince(last) >= 86_400 else { return }
        busy = true
        defer { busy = false }
        do {
            guard let passphrase = try BackupPassphraseStore.read() else { throw BackupFailure.invalid("Automatic backup passphrase is unavailable in Keychain.") }
            let include = UserDefaults.standard.bool(forKey: includeCredentialKey)
            let credential = include ? try CatalogCredentialStore.read() : nil
            try await service.prepareAutomaticDirectory(automaticDirectory)
            let stamp = ISO8601DateFormatter().string(from: Date()).replacingOccurrences(of: ":", with: "-")
            let url = automaticDirectory.appendingPathComponent("mellotv-auto-\(stamp)-\(UUID().uuidString).mellobackup")
            try await service.create(to: url, passphrase: passphrase, catalogCredential: credential)
            UserDefaults.standard.set(Date(), forKey: lastBackupKey)
            var owned = UserDefaults.standard.stringArray(forKey: ownedBackupsKey) ?? []
            owned.append(url.lastPathComponent)
            // Save the ownership record before deleting anything; unrelated files are never rotated.
            UserDefaults.standard.set(owned, forKey: ownedBackupsKey)
            let excess = Array(owned.dropLast(7))
            try await service.removeOwnedBackups(excess, in: automaticDirectory)
            owned = Array(owned.suffix(7))
            UserDefaults.standard.set(owned, forKey: ownedBackupsKey)
            message = "Automatic backup saved."
        } catch { message = "Automatic backup failed: \(error.localizedDescription)" }
    }
    func restore(from url: URL, passphrase: String, restoreCredential: Bool) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        do {
            let backup = try await service.validate(file: url, passphrase: passphrase)
            let wasRunning = isServerRunning()
            guard await stopServer() else { throw BackupFailure.invalid("The server did not stop. Restore was not started.") }
            do {
                try await service.restore(backup)
            } catch {
                if case .some(.rollbackIncomplete(_)) = error as? BackupFailure { message = error.localizedDescription; return }
                if wasRunning { startServer() }
                throw error
            }
            if restoreCredential, let credential = backup.catalogCredential {
                do { try CatalogCredentialStore.save(credential) }
                catch {
                    if wasRunning { startServer() }
                    message = "Household restored, but the personal TMDB credential couldn’t be saved. Update it in Settings."
                    return
                }
            }
            if wasRunning { startServer() }
            message = "Backup restored. A private rollback copy of the previous files was kept in the data folder."
        } catch { message = error.localizedDescription }
    }
}
