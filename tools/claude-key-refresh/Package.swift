// swift-tools-version:5.9
import PackageDescription

// claude-key-refresh is a macOS-only host helper (Option 3): it mirrors the
// Claude Code OAuth credential out of the login Keychain into a file that a
// Linux container can read through a mounted ~/.claude. It is intentionally
// outside the Rust workspace and is never built into a container image.
let package = Package(
    name: "claude-key-refresh",
    platforms: [.macOS(.v13)],
    dependencies: [
        .package(url: "https://github.com/apple/swift-argument-parser", from: "1.8.2"),
        // SwiftTUI's only release tag (0.1.0) predates most of the views we use,
        // so pin the modern main revision for a reproducible build.
        .package(
            url: "https://github.com/rensbreur/SwiftTUI",
            revision: "537133031bc2b2731048d00748c69700e1b48185"
        ),
        // colorized terminal text, used to render a styled --help screen.
        .package(url: "https://github.com/onevcat/Rainbow", from: "4.2.1"),
    ],
    targets: [
        .executableTarget(
            name: "claude-key-refresh",
            dependencies: [
                .product(name: "ArgumentParser", package: "swift-argument-parser"),
                .product(name: "SwiftTUI", package: "SwiftTUI"),
                .product(name: "Rainbow", package: "Rainbow"),
            ]
        )
    ]
)
