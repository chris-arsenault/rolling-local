import AVFoundation
import Combine
import CoreLocation
import Foundation
import UIKit

struct CameraDraft: Identifiable {
    let id = UUID()
    let image: UIImage
    let capturedAt = Date()
}

private struct StartSharing: Encodable { let id: UUID; let fix: Fix }
private struct UploadSize: Encodable { let bytes: Int }
private struct PublishMenu: Encodable { let id: UUID; let capturedAt: Date }

@MainActor final class OperatorModel: NSObject, ObservableObject, CLLocationManagerDelegate {
    @Published private(set) var profile: Profile?
    @Published private(set) var registration: Registration?
    @Published private(set) var sharing: Sharing?
    @Published private(set) var signedIn = false
    @Published private(set) var busy = false
    @Published private(set) var starting = false
    @Published private(set) var message: String?
    @Published var showCamera = false
    @Published var draft: CameraDraft?
    @Published var showSettings = false

    private let api = API()
    private let login = Login()
    private let manager = CLLocationManager()
    private var config: LoginConfig?
    private var restored = false
    private var desired: UUID?
    private var publishing = false
    private var expiry: Task<Void, Never>?

    override init() {
        super.init()
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyHundredMeters
        manager.activityType = .otherNavigation
        manager.allowsBackgroundLocationUpdates = true
        manager.showsBackgroundLocationIndicator = true
        manager.pausesLocationUpdatesAutomatically = false
    }

    func restore() async {
        guard !restored else { return }
        defer { restored = true }
        guard login.canRestore else {
            refreshWidget()
            return
        }
        await operation {
            try await load()
        }
    }

    func signIn() async {
        await operation {
            let config = try await configuration()
            try await login.signIn(config: config)
            try await load()
        }
    }

    func signOut() async {
        await stop()
        login.clear()
        signedIn = false
        profile = nil
        registration = nil
        refreshWidget()
    }

    func refreshRegistration() async {
        await operation { try await load() }
    }

    var editableSettings: Settings {
        if let profile { return profile.settings }
        var settings = Settings()
        settings.name = registration?.name ?? ""
        settings.cuisine = registration?.cuisine ?? ""
        return settings
    }

    func handleWidget() async {
        guard restored else { return }
        for action in WidgetActions.take() {
            switch action {
            case .sharing: await toggleSharing()
            case .camera: await openCamera()
            }
        }
    }

    func toggleSharing() async {
        if desired != nil || sharing != nil {
            await stop()
            return
        }
        guard profile != nil else {
            message = setupMessage(action: "share your location")
            return
        }
        message = nil
        desired = UUID()
        starting = true
        beginLocation()
        let attempt = desired
        Task {
            try? await Task.sleep(for: .seconds(30))
            if starting && desired == attempt {
                endLocal("Could not get a current location. Try again outdoors.")
            }
        }
    }

    func save(_ settings: Settings) async {
        await operation {
            let result: OperatorView = try await authorized("operator/settings", method: "PUT", body: settings)
            apply(result)
            if result.session == nil, desired != nil {
                endLocal("Sharing stopped when the operating settings changed.")
            }
            showSettings = false
        }
    }

    func openCamera() async {
        guard profile != nil else {
            message = setupMessage(action: "publish a menu")
            return
        }
        guard UIImagePickerController.isSourceTypeAvailable(.camera),
              await AVCaptureDevice.requestAccess(for: .video) else {
            message = "Allow camera access in Settings or open Rolling Local on a device with a camera."
            return
        }
        showCamera = true
    }

    func publish(_ draft: CameraDraft) async {
        await operation {
            let image = draft.image
            let ratio = min(1, 1800 / max(image.size.width, image.size.height))
            let size = CGSize(width: image.size.width * ratio, height: image.size.height * ratio)
            let format = UIGraphicsImageRendererFormat()
            format.scale = 1
            let preview = UIGraphicsImageRenderer(size: size, format: format).image { _ in
                image.draw(in: CGRect(origin: .zero, size: size))
            }
            guard let jpeg = preview.jpegData(compressionQuality: 0.85) else {
                throw ServerError(code: 0, message: "Could not open the photo.")
            }
            let ticket: UploadTicket = try await authorized("operator/menu/upload", method: "POST", body: UploadSize(bytes: jpeg.count))
            try await api.upload(jpeg, to: ticket.url)
            let _: Menu = try await authorized("operator/menu/publish", method: "POST",
                body: PublishMenu(id: ticket.id, capturedAt: draft.capturedAt))
            self.draft = nil
            try await load()
            message = "Menu published."
        }
    }

    private func configuration() async throws -> LoginConfig {
        if let config { return config }
        let loaded: LoginConfig = try await api.request("public/config")
        config = loaded
        return loaded
    }

    func menuResults() async throws -> MenuResults {
        try await authorized("operator/menu/extraction")
    }

    func saveMenu(_ correction: MenuCorrection) async throws -> MenuResults {
        try await authorized("operator/menu/extraction", method: "PUT", body: correction)
    }

    func retryMenu(_ extraction: MenuExtraction) async throws -> MenuResults {
        try await authorized("operator/menu/extraction/retry", method: "POST",
            body: ["menuId": extraction.menuId, "revision": extraction.revision])
    }

    private func authorized<T: Decodable>(_ path: String, method: String = "GET", body: (any Encodable)? = nil) async throws -> T {
        let config = try await configuration()
        let token = try await login.token(config: config)
        return try await api.request(path, method: method, bearer: token, body: body)
    }

    private func load() async throws {
        let result: OperatorView = try await authorized("operator")
        signedIn = true
        apply(result)
    }

    private func apply(_ result: OperatorView) {
        profile = result.profile
        registration = result.registration
        sharing = result.session
        if let session = result.session { scheduleExpiry(session) }
        refreshWidget()
    }

    private func refreshWidget() {
        WidgetStatus(name: registration?.name ?? "Rolling Local", expiresAt: sharing?.expiresAt).save()
    }

    private func setupMessage(action: String) -> String {
        guard signedIn else { return "Sign in to \(action)." }
        if registration?.status == .approved {
            showSettings = true
            return "Set up your operating hours and area first."
        }
        return "Your truck needs approval before you can \(action)."
    }

    private func operation(_ work: () async throws -> Void) async {
        guard !busy else { return }
        busy = true
        message = nil
        defer { busy = false }
        do {
            try await work()
        } catch {
            message = error.localizedDescription
        }
    }

    private func beginLocation() {
        guard desired != nil else { return }
        switch manager.authorizationStatus {
        case .notDetermined: manager.requestWhenInUseAuthorization()
        case .authorizedWhenInUse, .authorizedAlways: manager.startUpdatingLocation()
        case .denied, .restricted: endLocal("Allow location access in Settings to share your truck.")
        @unknown default: endLocal("Location access is unavailable.")
        }
    }

    nonisolated func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        Task { @MainActor in
            if (manager.authorizationStatus == .denied || manager.authorizationStatus == .restricted),
               desired != nil {
                await stop(reason: "Sharing stopped when location access was removed.")
            } else {
                beginLocation()
            }
        }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let location = locations.last else { return }
        Task { @MainActor in await locationUpdated(location) }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        if (error as? CLError)?.code == .denied {
            Task { @MainActor in await stop(reason: "Sharing stopped when location became unavailable.") }
        }
    }

    private func locationUpdated(_ location: CLLocation) async {
        guard let id = desired, let profile, location.horizontalAccuracy >= 0,
              abs(location.timestamp.timeIntervalSinceNow) <= 120 else { return }
        if let sharing, Date() >= min(sharing.expiresAt, sharing.closesAt) {
            await stop(reason: "The sharing session expired.")
            return
        }
        let normal = profile.settings.area
        let event = profile.settings.event
        let scope = event.flatMap { $0.expiresAt > Date() ? $0.area : nil } ?? normal
        let center = CLLocation(latitude: scope.center.latitude, longitude: scope.center.longitude)
        if location.distance(from: center) + location.horizontalAccuracy > scope.radiusMeters {
            await stop(reason: "Sharing stopped outside your operating area.")
            return
        }
        if let sharing, sharing.publishedAt.timeIntervalSinceNow > -600 { return }
        guard !publishing else { return }
        publishing = true
        defer { publishing = false }
        let fix = Fix(position: Position(latitude: location.coordinate.latitude, longitude: location.coordinate.longitude),
            accuracyMeters: location.horizontalAccuracy, recordedAt: location.timestamp)
        do {
            let result: Sharing
            if sharing == nil {
                result = try await authorized("operator/sessions", method: "POST", body: StartSharing(id: id, fix: fix))
            } else {
                result = try await authorized("operator/sessions/\(id.uuidString)/fix", method: "PUT", body: fix)
            }
            guard desired == id else {
                let _: OperatorView? = try? await authorized("operator/sessions/\(id.uuidString)", method: "DELETE")
                return
            }
            starting = false
            sharing = result
            message = nil
            scheduleExpiry(result)
            refreshWidget()
        } catch let error as ServerError where [400, 401, 403, 409].contains(error.code) {
            await stop(reason: error.localizedDescription)
        } catch {
            if sharing == nil {
                endLocal("Sharing could not start. \(error.localizedDescription)")
            } else {
                message = "Update failed. The last position expires at the time shown."
            }
        }
    }

    private func scheduleExpiry(_ session: Sharing) {
        expiry?.cancel()
        let delay = max(0, session.expiresAt.timeIntervalSinceNow)
        expiry = Task {
            do {
                try await Task.sleep(for: .seconds(delay))
                if sharing?.id == session.id { endLocal("Sharing expired.") }
            } catch is CancellationError {
                return
            } catch {
                endLocal("Sharing expired.")
            }
        }
    }

    private func stop(reason: String? = nil) async {
        let id = sharing?.id ?? desired
        endLocal(reason)
        guard let id else { return }
        do {
            let _: OperatorView = try await authorized("operator/sessions/\(id.uuidString)", method: "DELETE")
        } catch {
            message = "Phone sharing stopped. The last public position will expire if the server cannot be reached."
        }
    }

    private func endLocal(_ reason: String?) {
        desired = nil
        starting = false
        sharing = nil
        expiry?.cancel()
        expiry = nil
        manager.stopUpdatingLocation()
        message = reason
        refreshWidget()
    }
}
