import Foundation
import SwiftTUI

// live dashboard: status, counters, fingerprint, and a recent-change feed.
struct DashboardView: View {
    @ObservedObject var model: RefreshModel

    // sections keep each VStack within SwiftTUI's 10-child ViewBuilder limit.
    var body: some View {
        VStack(alignment: .leading) {
            header
            config
            stats
            feed
        }
        .padding()
    }

    private var header: some View {
        VStack(alignment: .leading) {
            Text(" claude-key-refresh ")
                .bold()
                .foregroundColor(.black)
                .background(.cyan)
            row("status", model.statusText, color(for: model.statusKind))
        }
    }

    private var config: some View {
        VStack(alignment: .leading) {
            Divider()
            row("service", model.config.service, .white)
            row("destination", model.config.destination.path, .white)
            row("interval", "\(Int(model.config.interval))s", .white)
        }
    }

    private var stats: some View {
        VStack(alignment: .leading) {
            Divider()
            row(
                "keychain",
                model.keychainPresent ? "present · \(model.tokenBytes) bytes" : "not found",
                model.keychainPresent ? .green : .red
            )
            row("fingerprint", model.fingerprint, .white)
            row("checks", "\(model.checkCount)", .white)
            row("writes", "\(model.writeCount)", .white)
            row("last check", stamp(model.lastCheck), .white)
            row("last write", stamp(model.lastChange), .white)
        }
    }

    private var feed: some View {
        VStack(alignment: .leading) {
            Divider()
            Text("recent changes").foregroundColor(.gray)
            changeFeed
            Divider()
            Text("ctrl-c to quit").foregroundColor(.gray)
        }
    }

    @ViewBuilder
    private var changeFeed: some View {
        if model.events.isEmpty {
            Text("  (nothing captured yet)").foregroundColor(.gray)
        } else {
            ForEach(model.events) { event in
                HStack {
                    Text(stampTime(event.time)).foregroundColor(.gray)
                    Text(glyph(event.kind)).foregroundColor(color(for: event.kind))
                    Text(event.detail).foregroundColor(.white)
                }
            }
        }
    }

    private func row(_ label: String, _ value: String, _ valueColor: Color) -> some View {
        HStack {
            Text(pad(label, 13)).foregroundColor(.gray)
            Text(value).foregroundColor(valueColor)
        }
    }
}

private func pad(_ value: String, _ width: Int) -> String {
    value.count >= width ? value : value + String(repeating: " ", count: width - value.count)
}

private func glyph(_ kind: ChangeEvent.Kind) -> String {
    switch kind {
    case .seeded: return "+"
    case .changed: return "~"
    case .drift: return "="
    case .error: return "x"
    case .info: return "."
    }
}

private func color(for kind: ChangeEvent.Kind) -> Color {
    switch kind {
    case .seeded: return .green
    case .changed: return .cyan
    case .drift: return .yellow
    case .error: return .red
    case .info: return .green
    }
}

private let clock: DateFormatter = {
    let formatter = DateFormatter()
    formatter.dateFormat = "HH:mm:ss"
    return formatter
}()

private func stampTime(_ date: Date) -> String { clock.string(from: date) }
private func stamp(_ date: Date?) -> String { date.map(stampTime) ?? "—" }
