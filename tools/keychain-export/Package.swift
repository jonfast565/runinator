// swift-tools-version:5.9
import PackageDescription

// keychain-export is a macOS-only host helper: it reads a secret from the login
// Keychain and writes it to stdout (or a file) in raw/base64/hex/json form, so a
// separate rotator can deliver it to Linux containers. it does one thing —
// native Keychain extraction — and has no external dependencies. it is outside
// the Rust workspace and is never built into a container image.
let package = Package(
    name: "keychain-export",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(name: "keychain-export")
    ]
)
