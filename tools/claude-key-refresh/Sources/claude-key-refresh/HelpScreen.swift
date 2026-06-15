import Foundation
import Rainbow

// a styled, colorized --help screen. mirrors the @Option/@Flag definitions in
// Entry.swift; keep the two in sync.
enum HelpScreen {
    private struct Item {
        let flag: String
        let arg: String
        let help: String
    }

    private static let options: [Item] = [
        Item(flag: "-s, --service", arg: "<name>", help: "Keychain generic-password service (default: Claude Code-credentials)"),
        Item(flag: "    --account", arg: "<name>", help: "Optional Keychain account to disambiguate the item"),
        Item(flag: "-o, --output", arg: "<path>", help: "Destination file (default: ~/.claude/.credentials.json)"),
        Item(flag: "-i, --interval", arg: "<secs>", help: "Seconds between checks in watch mode (default: 15)"),
        Item(flag: "    --once", arg: "", help: "Run a single check, write if changed, then exit (cron/launchd)"),
        Item(flag: "    --plain", arg: "", help: "Watch without the TUI; log one line per check"),
        Item(flag: "    --clean", arg: "", help: "Remove the mirrored destination credentials file, then exit"),
        Item(flag: "-h, --help", arg: "", help: "Show this help screen"),
    ]

    private static let examples: [(String, String)] = [
        ("claude-key-refresh", "live dashboard, checks every 15s"),
        ("claude-key-refresh -i 30", "dashboard with a 30s interval"),
        ("claude-key-refresh --once", "one-shot for cron/launchd"),
        ("claude-key-refresh --plain", "headless watch, one log line per check"),
        ("claude-key-refresh --clean", "delete the mirrored credentials file"),
    ]

    static func render() -> String {
        let columnWidth = options.map { $0.flag.count + $0.arg.count + 1 }.max() ?? 24
        var lines: [String] = []

        lines.append("")
        lines.append("  claude-key-refresh ".black.onCyan.bold)
        lines.append("  Mirror the Claude Code login from the macOS Keychain into a file,".white)
        lines.append("  writing only when it changes.".white)
        lines.append("")

        lines.append(section("USAGE"))
        lines.append("  " + "claude-key-refresh".green + " " + "[options]".lightBlack)
        lines.append("")

        lines.append(section("OPTIONS"))
        for item in options {
            let label = item.arg.isEmpty ? item.flag : "\(item.flag) \(item.arg)"
            let padded = label.padding(toLength: max(columnWidth + 2, label.count), withPad: " ", startingAt: 0)
            lines.append("  " + padded.green + "  " + item.help.white)
        }
        lines.append("")

        lines.append(section("EXAMPLES"))
        let exampleWidth = examples.map { $0.0.count }.max() ?? 24
        for (command, note) in examples {
            let padded = command.padding(toLength: exampleWidth, withPad: " ", startingAt: 0)
            lines.append("  " + padded.cyan + "   " + note.lightBlack)
        }
        lines.append("")

        lines.append(section("NOTES"))
        lines.append("  " + "•".yellow + " First Keychain read shows a one-time macOS \"Always Allow\" prompt;".white)
        lines.append("    click it interactively before any unattended --once run.".lightBlack)
        lines.append("  " + "•".yellow + " The secret is never printed — only a short SHA-256 fingerprint.".white)
        lines.append("")

        return lines.joined(separator: "\n")
    }

    private static func section(_ title: String) -> String {
        title.yellow.bold
    }
}
