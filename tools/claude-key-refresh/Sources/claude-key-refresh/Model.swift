import Foundation
import Combine

// resolved runtime configuration shared by the watch loop and the views.
struct Config {
    var service: String
    var account: String?
    var destination: URL
    var interval: TimeInterval
}

// a single captured transition worth showing in the dashboard.
struct ChangeEvent: Identifiable {
    enum Kind {
        case seeded
        case changed
        case drift
        case error
        case info
    }

    let id = UUID()
    let time: Date
    let kind: Kind
    let detail: String
}

// observable state driving the TUI; one tick = one keychain check.
final class RefreshModel: ObservableObject {
    let config: Config

    @Published private(set) var checkCount = 0
    @Published private(set) var writeCount = 0
    @Published private(set) var lastCheck: Date?
    @Published private(set) var lastChange: Date?
    @Published private(set) var keychainPresent = false
    @Published private(set) var tokenBytes = 0
    @Published private(set) var fingerprint = "—"
    @Published private(set) var statusKind: ChangeEvent.Kind = .info
    @Published private(set) var statusText = "starting…"
    @Published private(set) var events: [ChangeEvent] = []

    private var lastHash: String?
    private var lastStatusWasError = false
    private let maxEvents = 8

    init(config: Config) {
        self.config = config
    }

    // performs one check and writes the destination only when the secret changed
    // (or the destination drifted out of sync). returns true if it wrote a file.
    @discardableResult
    func tick() -> Bool {
        checkCount += 1
        lastCheck = Date()

        do {
            let data = try Keychain.read(service: config.service, account: config.account)
            keychainPresent = true
            tokenBytes = data.count

            let hash = Support.sha256Hex(data)
            fingerprint = String(hash.prefix(12))

            let destinationData = try? Data(contentsOf: config.destination)
            let destinationHash = destinationData.map(Support.sha256Hex)

            let keychainChanged = (hash != lastHash)
            let destinationDrifted = (destinationHash != hash)

            guard keychainChanged || destinationDrifted else {
                setStatus(.info, "in sync")
                return false
            }

            try Support.writeAtomic(data, to: config.destination)
            writeCount += 1
            lastChange = Date()

            let kind: ChangeEvent.Kind
            let reason: String
            if lastHash == nil {
                kind = .seeded
                reason = "initial seed"
            } else if keychainChanged {
                kind = .changed
                reason = "keychain rotated"
            } else {
                kind = .drift
                reason = "destination drifted"
            }

            lastHash = hash
            record(kind, "\(reason) → wrote \(data.count) bytes [\(fingerprint)]")
            setStatus(kind, "wrote credentials")
            return true
        } catch let error as KeychainError {
            handleError(error.description, missing: isMissing(error))
            return false
        } catch {
            handleError("\(error)", missing: false)
            return false
        }
    }

    private func isMissing(_ error: KeychainError) -> Bool {
        if case .notFound = error { return true }
        return false
    }

    // only logs a fresh error event on transition so a persistent failure
    // (e.g. denied keychain access) does not flood the change list.
    private func handleError(_ message: String, missing: Bool) {
        keychainPresent = false
        statusKind = .error
        statusText = missing ? "keychain item not found" : "error"
        if !lastStatusWasError {
            record(.error, message)
        }
        lastStatusWasError = true
    }

    private func setStatus(_ kind: ChangeEvent.Kind, _ text: String) {
        statusKind = kind
        statusText = text
        lastStatusWasError = false
    }

    private func record(_ kind: ChangeEvent.Kind, _ detail: String) {
        events.insert(ChangeEvent(time: Date(), kind: kind, detail: detail), at: 0)
        if events.count > maxEvents {
            events.removeLast(events.count - maxEvents)
        }
    }
}
