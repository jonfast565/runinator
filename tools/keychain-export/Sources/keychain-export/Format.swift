import Foundation

// how the extracted secret bytes are rendered before writing.
enum OutputFormat: String {
    case raw     // the secret bytes, unchanged (e.g. the credential JSON itself)
    case base64  // base64 of the secret bytes
    case hex     // lowercase hex of the secret bytes
    case json    // a JSON object: {"service","account","value","encoding"}
}

enum Format {
    // encodes the secret for output. text formats get a trailing newline so they
    // are terminal- and file-friendly; raw stays byte-exact for downstream tools.
    static func encode(
        _ data: Data,
        as format: OutputFormat,
        service: String,
        account: String?
    ) throws -> Data {
        switch format {
        case .raw:
            return data
        case .base64:
            return line(data.base64EncodedString())
        case .hex:
            return line(data.map { String(format: "%02x", $0) }.joined())
        case .json:
            return try json(data, service: service, account: account)
        }
    }

    private static func line(_ text: String) -> Data {
        Data((text + "\n").utf8)
    }

    // emits the value as a UTF-8 string when the bytes decode cleanly, otherwise
    // base64 with an explicit encoding marker so the consumer can recover bytes.
    private static func json(_ data: Data, service: String, account: String?) throws -> Data {
        var object: [String: Any] = ["service": service]
        object["account"] = account ?? NSNull()
        if let text = String(data: data, encoding: .utf8) {
            object["value"] = text
            object["encoding"] = "utf8"
        } else {
            object["value"] = data.base64EncodedString()
            object["encoding"] = "base64"
        }
        let json = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
        return line(String(decoding: json, as: UTF8.self))
    }
}
