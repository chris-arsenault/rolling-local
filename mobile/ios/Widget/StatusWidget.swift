import SwiftUI
import WidgetKit

struct StatusEntry: TimelineEntry {
    var date: Date
    var status: WidgetStatus

    var sharing: Bool {
        status.expiresAt.map { date < $0 } ?? false
    }
}

struct StatusProvider: TimelineProvider {
    func placeholder(in context: Context) -> StatusEntry {
        StatusEntry(date: .now, status: WidgetStatus(name: "Rolling Local", expiresAt: nil))
    }

    func getSnapshot(in context: Context, completion: @escaping (StatusEntry) -> Void) {
        completion(StatusEntry(date: .now, status: .load()))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<StatusEntry>) -> Void) {
        let current = StatusEntry(date: .now, status: .load())
        let entries = current.status.expiresAt.flatMap { expiry in
            expiry > current.date ? [current, StatusEntry(date: expiry, status: current.status)] : nil
        } ?? [current]
        completion(Timeline(entries: entries, policy: .never))
    }
}

@main struct OperatorStatusWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "operator-status", provider: StatusProvider()) { entry in
            VStack(alignment: .leading, spacing: 8) {
                Text(entry.status.name).font(.headline).lineLimit(1)
                Text(entry.sharing ? "Sharing" : "Offline")
                    .font(.caption)
                    .foregroundStyle(entry.sharing ? Color.green : Color.secondary)
                Spacer(minLength: 0)
                HStack(spacing: 10) {
                    Button(intent: ToggleSharing()) {
                        Image(systemName: "power").frame(minWidth: 28, minHeight: 28)
                    }.accessibilityLabel(entry.sharing ? "Stop sharing" : "Start sharing")
                    Button(intent: OpenMenuCamera()) {
                        Image(systemName: "camera").frame(minWidth: 28, minHeight: 28)
                    }.accessibilityLabel("Open menu camera")
                }.buttonStyle(.bordered)
            }
            .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Rolling Local")
        .description("Toggle sharing or open your menu camera.")
        .supportedFamilies([.systemSmall])
    }
}
