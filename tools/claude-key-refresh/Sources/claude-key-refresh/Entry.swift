import Foundation
import ArgumentParser
import SwiftTUI

// default destination is the Linux location claude reads on the container side.
private func defaultOutput() -> String {
    FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".claude/.credentials.json")
        .path
}

// custom entry point: render the styled Rainbow help for -h/--help, otherwise
// hand off to ArgumentParser. a bare invocation still launches the dashboard.
@main
enum Main {
    static func main() {
        let arguments = Array(CommandLine.arguments.dropFirst())
        if arguments.contains("-h") || arguments.contains("--help") {
            print(HelpScreen.render())
            return
        }
        ClaudeKeyRefresh.main()
    }
}

struct ClaudeKeyRefresh: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "claude-key-refresh",
        abstract: "Mirror the Claude Code login from the macOS Keychain into ~/.claude/.credentials.json, writing only when it changes.",
        discussion: """
        On macOS the Claude Code OAuth login lives in the login Keychain, not in a
        file. Linux containers read it from ~/.claude/.credentials.json. This tool
        copies the Keychain secret into that file so a mounted ~/.claude carries the
        login, and re-copies it whenever the Keychain value rotates.
        """
    )

    @Option(name: .shortAndLong, help: "Keychain generic-password service name.")
    var service: String = "Claude Code-credentials"

    @Option(name: .long, help: "Optional Keychain account to disambiguate the item.")
    var account: String?

    @Option(name: .shortAndLong, help: "Destination credentials file.")
    var output: String = defaultOutput()

    @Option(name: .shortAndLong, help: "Seconds between checks in watch mode.")
    var interval: Double = 15

    @Flag(name: .long, help: "Run a single check, write if changed, then exit (for cron/launchd).")
    var once = false

    @Flag(name: .long, help: "Watch without the TUI; log one line per check.")
    var plain = false

    @Flag(name: .long, help: "Remove the mirrored destination credentials file, then exit.")
    var clean = false

    func validate() throws {
        guard interval >= 1 else {
            throw ValidationError("interval must be at least 1 second")
        }
    }

    func run() throws {
        let config = Config(
            service: service,
            account: account,
            destination: URL(fileURLWithPath: (output as NSString).expandingTildeInPath),
            interval: interval
        )
        if clean {
            try cleanup(config.destination)
            return
        }

        let model = RefreshModel(config: config)

        if once {
            let wrote = model.tick()
            logLine(model, wrote: wrote)
            if model.statusKind == .error { throw ExitCode(1) }
            return
        }

        if plain {
            runPlainWatch(model)
        }

        runDashboard(model)
    }

    // logs one line per check; never returns.
    private func runPlainWatch(_ model: RefreshModel) -> Never {
        let timer = DispatchSource.makeTimerSource(queue: .main)
        timer.schedule(deadline: .now(), repeating: model.config.interval)
        timer.setEventHandler {
            let wrote = model.tick()
            logLine(model, wrote: wrote)
        }
        timer.resume()
        dispatchMain()
    }

    // drives the SwiftTUI dashboard with a main-queue timer.
    private func runDashboard(_ model: RefreshModel) -> Never {
        let timer = DispatchSource.makeTimerSource(queue: .main)
        timer.schedule(deadline: .now() + model.config.interval, repeating: model.config.interval)
        timer.setEventHandler {
            model.tick()
        }
        timer.resume()

        // seed the view with an immediate first check.
        model.tick()
        Application(rootView: DashboardView(model: model)).start()

        // start() runs the event loop and does not return.
        dispatchMain()
    }

    // removes the mirrored credentials file; absence is treated as success.
    private func cleanup(_ url: URL) throws {
        let fileManager = FileManager.default
        guard fileManager.fileExists(atPath: url.path) else {
            print("nothing to remove: \(url.path)")
            return
        }
        do {
            try fileManager.removeItem(at: url)
            print("removed \(url.path)")
        } catch {
            FileHandle.standardError.write(Data("failed to remove \(url.path): \(error)\n".utf8))
            throw ExitCode(1)
        }
    }

    private func logLine(_ model: RefreshModel, wrote: Bool) {
        let timestamp = ISO8601DateFormatter().string(from: Date())
        let state = model.keychainPresent ? "ok" : "missing"
        let action = wrote ? "WROTE" : "noop"
        print("[\(timestamp)] \(state) \(action) checks=\(model.checkCount) writes=\(model.writeCount) fp=\(model.fingerprint) — \(model.statusText)")
    }
}
