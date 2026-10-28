import Foundation
import Security

/// The catalog credential belongs to this Mac, never to the app bundle or UserDefaults.
enum CatalogCredentialStore {
    private static let service = "com.tinymellow.mellotv.catalog"
    private static let account = "tmdb"

    private static var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account,
         kSecAttrSynchronizable as String: false]
    }

    enum CredentialError: LocalizedError {
        case keychain(OSStatus)
        case invalid
        case rejected
        case unavailable

        var errorDescription: String? {
            switch self {
            case .keychain:
                return "Couldn't access the TMDB credential in Keychain. Unlock your login keychain and try again."
            case .invalid:
                return "Paste a TMDB API key or API Read Access Token from your TMDB account."
            case .rejected:
                return "TMDB rejected this credential. Check your API key or Read Access Token and try again."
            case .unavailable:
                return "Couldn't validate the credential with TMDB. Check your connection and try again."
            }
        }
    }

    static func read() throws -> String? {
        var request = query
        request[kSecReturnData as String] = true
        request[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(request as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data,
              let value = String(data: data, encoding: .utf8) else {
            throw CredentialError.keychain(status)
        }
        return value
    }

    static func save(_ value: String) throws {
        let attributes: [String: Any] = [kSecValueData as String: Data(value.utf8)]
        let status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            var item = query.merging(attributes) { _, new in new }
            item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            let added = SecItemAdd(item as CFDictionary, nil)
            guard added == errSecSuccess else { throw CredentialError.keychain(added) }
        } else if status != errSecSuccess {
            throw CredentialError.keychain(status)
        }
    }

    static func remove() throws {
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw CredentialError.keychain(status)
        }
    }

    static func isAPIKey(_ value: String) -> Bool {
        value.utf8.count == 32 && value.utf8.allSatisfy {
            (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0)
        }
    }

    static func validate(_ input: String) async throws -> String {
        let value = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty, value.utf8.count <= 4096,
              value.utf8.allSatisfy({ $0 > 32 && $0 < 127 }) else {
            throw CredentialError.invalid
        }
        var components = URLComponents(string: "https://api.themoviedb.org/3/authentication")!
        if isAPIKey(value) { components.queryItems = [URLQueryItem(name: "api_key", value: value)] }
        var request = URLRequest(url: components.url!)
        if !isAPIKey(value) { request.setValue("Bearer \(value)", forHTTPHeaderField: "Authorization") }
        request.timeoutInterval = 15
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        let configuration = URLSessionConfiguration.ephemeral
        configuration.urlCache = nil
        let session = URLSession(configuration: configuration)
        defer { session.invalidateAndCancel() }
        do {
            let (_, response) = try await session.data(for: request)
            guard let response = response as? HTTPURLResponse else { throw CredentialError.unavailable }
            if response.statusCode == 401 || response.statusCode == 403 { throw CredentialError.rejected }
            guard (200..<300).contains(response.statusCode) else { throw CredentialError.unavailable }
            return value
        } catch let error as CredentialError {
            throw error
        } catch {
            // URLSession errors can contain the request URL; never surface it with an API key.
            throw CredentialError.unavailable
        }
    }
}
