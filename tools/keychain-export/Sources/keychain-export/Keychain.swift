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

// the keychain item class to query.
enum ItemKind: String {
    case generic
    case internet

    var secClass: CFString {
        switch self {
        case .generic: return kSecClassGenericPassword
        case .internet: return kSecClassInternetPassword
        }
    }

    // generic passwords key off service; internet passwords key off server.
    var primaryAttribute: CFString {
        switch self {
        case .generic: return kSecAttrService
        case .internet: return kSecAttrServer
        }
    }
}

enum Keychain {
    // reads a password secret blob for the given item kind, service/server, and
    // optional account.
    static func read(service: String, account: String?, kind: ItemKind) throws -> Data {
        var query: [String: Any] = [
            kSecClass as String: kind.secClass,
            kind.primaryAttribute as String: service,
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
