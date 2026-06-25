import Foundation

// keychain-export: read a secret from the macOS login Keychain and emit
// it. the defaults target the Claude Code OAuth login (a generic password under
// "Claude Code-credentials", raw bytes to stdout) so the rotator can capture it,
// but any generic- or internet-password item can be extracted to any path in
// raw/base64/hex/json form. exit codes: 0 wrote a credential, 3 the keychain
// item was not found, 1 any other error. the secret itself is never logged —
// only a short fingerprint on stderr.

struct Options {
    var service = "Claude Code-credentials"
    var account: String?
    var output: String?
    var kind: ItemKind = .generic
    var format: OutputFormat = .raw
    var quiet = false
}

enum ExitStatus: Int32 {
    case ok = 0
    case error = 1
    case notFound = 3
}

private func usage() -> String {
    """
    keychain-export — read a secret from the macOS login Keychain.

    USAGE:
      keychain-export [options]

    OPTIONS:
      -s, --service <name>   Keychain service (generic) or server (internet)
                             (default: "Claude Code-credentials")
          --account <name>   optional Keychain account to disambiguate the item
          --kind <kind>      item class: generic | internet (default: generic)
      -f, --format <fmt>     output encoding: raw | base64 | hex | json
                             (default: raw — the secret bytes unchanged)
      -o, --output <path>    write to this file (0600, atomic); default: stdout
      -q, --quiet            suppress the stderr fingerprint line
      -h, --help             show this help

    EXIT CODES:
      0  wrote a credential   3  keychain item not found   1  other error
    """
}

// minimal hand-rolled parser; the tool intentionally carries no dependencies.
private func parse(_ args: [String]) -> Options? {
    var options = Options()
    var index = 0
    func next(_ flag: String) -> String? {
        index += 1
        guard index < args.count else {
            FileHandle.standardError.write(Data("missing value for \(flag)\n".utf8))
            return nil
        }
        return args[index]
    }
    while index < args.count {
        let arg = args[index]
        switch arg {
        case "-h", "--help":
            print(usage())
            exit(ExitStatus.ok.rawValue)
        case "-s", "--service":
            guard let value = next(arg) else { return nil }
            options.service = value
        case "--account":
            guard let value = next(arg) else { return nil }
            options.account = value
        case "--kind":
            guard let value = next(arg) else { return nil }
            guard let kind = ItemKind(rawValue: value) else {
                FileHandle.standardError.write(Data("invalid --kind: \(value) (expected generic|internet)\n".utf8))
                return nil
            }
            options.kind = kind
        case "-f", "--format":
            guard let value = next(arg) else { return nil }
            guard let format = OutputFormat(rawValue: value) else {
                FileHandle.standardError.write(Data("invalid --format: \(value) (expected raw|base64|hex|json)\n".utf8))
                return nil
            }
            options.format = format
        case "-o", "--output":
            guard let value = next(arg) else { return nil }
            options.output = value
        case "-q", "--quiet":
            options.quiet = true
        default:
            FileHandle.standardError.write(Data("unknown argument: \(arg)\n".utf8))
            return nil
        }
        index += 1
    }
    return options
}

@main
enum Main {
    static func main() {
        guard let options = parse(Array(CommandLine.arguments.dropFirst())) else {
            FileHandle.standardError.write(Data("\n\(usage())\n".utf8))
            exit(ExitStatus.error.rawValue)
        }

        let data: Data
        do {
            data = try Keychain.read(service: options.service, account: options.account, kind: options.kind)
        } catch let error as KeychainError {
            FileHandle.standardError.write(Data("\(error.description)\n".utf8))
            if case .notFound = error { exit(ExitStatus.notFound.rawValue) }
            exit(ExitStatus.error.rawValue)
        } catch {
            FileHandle.standardError.write(Data("\(error)\n".utf8))
            exit(ExitStatus.error.rawValue)
        }

        if !options.quiet {
            let fingerprint = String(Support.sha256Hex(data).prefix(12))
            FileHandle.standardError.write(Data("read \(data.count) bytes [\(fingerprint)]\n".utf8))
        }

        let payload: Data
        do {
            payload = try Format.encode(data, as: options.format, service: options.service, account: options.account)
        } catch {
            FileHandle.standardError.write(Data("format failed: \(error)\n".utf8))
            exit(ExitStatus.error.rawValue)
        }

        if let output = options.output {
            let url = URL(fileURLWithPath: (output as NSString).expandingTildeInPath)
            do {
                try Support.writeAtomic(payload, to: url)
            } catch {
                FileHandle.standardError.write(Data("write failed: \(error)\n".utf8))
                exit(ExitStatus.error.rawValue)
            }
        } else {
            FileHandle.standardOutput.write(payload)
        }
        exit(ExitStatus.ok.rawValue)
    }
}
