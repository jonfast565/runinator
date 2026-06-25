import Foundation
import Darwin
import CryptoKit

// failure writing the destination credentials file.
enum RefreshError: Error, CustomStringConvertible {
    case write(String)

    var description: String {
        switch self {
        case .write(let message): return message
        }
    }
}

enum Support {
    // hex sha256, used to fingerprint the secret without ever exposing it.
    static func sha256Hex(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    // writes data atomically with 0600 perms, creating parent dirs as needed.
    static func writeAtomic(_ data: Data, to url: URL) throws {
        let directory = url.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)

        let temp = directory.appendingPathComponent(".\(url.lastPathComponent).tmp.\(UUID().uuidString)")
        try data.write(to: temp, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: temp.path)

        // posix rename atomically replaces an existing destination in place.
        if rename(temp.path, url.path) != 0 {
            let reason = String(cString: strerror(errno))
            try? FileManager.default.removeItem(at: temp)
            throw RefreshError.write("rename failed: \(reason)")
        }
    }
}
