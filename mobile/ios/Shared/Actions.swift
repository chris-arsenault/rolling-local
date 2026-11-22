import AppIntents
import Foundation
import WidgetKit

enum WidgetAction {
    case sharing
    case camera
}

@MainActor enum WidgetActions {
    static let didEnqueue = Notification.Name("rolling-local.widget-action")
    private static var pending: [WidgetAction] = []

    static func enqueue(_ action: WidgetAction) {
        pending.append(action)
        NotificationCenter.default.post(name: didEnqueue, object: nil)
    }

    static func take() -> [WidgetAction] {
        defer { pending.removeAll() }
        return pending
    }
}

struct ToggleSharing: AppIntent {
    static let title: LocalizedStringResource = "Toggle sharing"
    static let openAppWhenRun = true

    @MainActor func perform() async throws -> some IntentResult {
        WidgetActions.enqueue(.sharing)
        return .result()
    }
}

struct OpenMenuCamera: AppIntent {
    static let title: LocalizedStringResource = "Open menu camera"
    static let openAppWhenRun = true

    @MainActor func perform() async throws -> some IntentResult {
        WidgetActions.enqueue(.camera)
        return .result()
    }
}

struct WidgetStatus: Codable {
    var name: String
    var expiresAt: Date?

    private static var defaults: UserDefaults? {
        UserDefaults(suiteName: "group.io.ahara.rollinglocal.operator")
    }

    static func load() -> Self {
        guard let data = defaults?.data(forKey: "widget-status"),
              let status = try? JSONDecoder().decode(Self.self, from: data) else {
            return Self(name: "Rolling Local", expiresAt: nil)
        }
        return status
    }

    func save() {
        Self.defaults?.set(try? JSONEncoder().encode(self), forKey: "widget-status")
        WidgetCenter.shared.reloadTimelines(ofKind: "operator-status")
    }
}
