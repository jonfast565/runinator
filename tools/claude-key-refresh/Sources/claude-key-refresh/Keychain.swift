import Foundation
import Security

// errors surfaced while reading the keychain, rendered for the dashboard/log.
enum KeychainError: Error, CustomStringConvertible {
    case notFound
    case unexpectedData
    case status(OSStatus)

    var description: String {
        switch self {
        case .notFound:
            return "keychain item not found"
        case .unexpectedData:
            return "keychain returned unexpected data"
        case .status(let status):
            let message = SecCopyErrorMessageString(status, nil) as String?
            return "keychain error: \(message ?? "OSStatus \(status)")"
        }
    }
}

enum Keychain {
    // reads a generic-password secret blob for the given service (and optional account).
    static func read(service: String, account: String?) throws -> Data {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]

        if let account, !account.isEmpty {
            query[kSecAttrAccount as String] = account
        }

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)

        guard status != errSecItemNotFound else { throw KeychainError.notFound }
        guard status == errSecSuccess else { throw KeychainError.status(status) }
        guard let data = item as? Data else { throw KeychainError.unexpectedData }

        return data
    }
}
