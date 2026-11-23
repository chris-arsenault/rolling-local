import SwiftUI
import UIKit

@main struct RollingLocalApp: App {
    @StateObject private var model = OperatorModel()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            HomeView(model: model)
                .tint(Color(hue: 0.43, saturation: 0.76, brightness: 0.40))
                .task {
                    await model.restore()
                    await model.handleWidget()
                }
                .onReceive(NotificationCenter.default.publisher(for: WidgetActions.didEnqueue)) { _ in
                    Task { await model.handleWidget() }
                }
                .onChange(of: scenePhase) { _, phase in
                    if phase == .active { Task { await model.handleWidget() } }
                }
        }
    }
}

struct HomeView: View {
    @ObservedObject var model: OperatorModel

    var body: some View {
        NavigationStack {
            Form {
                if !model.signedIn {
                    Section {
                        Text("Share your truck’s location and current menu with nearby customers.")
                        Button("Sign in with Ahara") { Task { await model.signIn() } }
                            .disabled(model.busy)
                    }
                } else if let profile = model.profile {
                    Section("Location") {
                        LabeledContent("Status", value: model.starting ? "Finding location" : model.sharing == nil ? "Offline" : "Sharing")
                        if let session = model.sharing {
                            LabeledContent("Last update", value: session.publishedAt.formatted(date: .omitted, time: .shortened))
                            LabeledContent("Expires without updates", value: session.expiresAt.formatted(date: .omitted, time: .shortened))
                        }
                        Button(model.sharing != nil || model.starting ? "Stop sharing" : "Start sharing") {
                            Task { await model.toggleSharing() }
                        }.buttonStyle(.borderedProminent)
                    }
                    Section("Menu") {
                        if let menu = profile.menu {
                            AsyncImage(url: menu.url) { image in
                                image.resizable().scaledToFit().frame(maxHeight: 200)
                            } placeholder: {
                                ProgressView()
                            }
                            LabeledContent("Photo taken", value: menu.capturedAt.formatted(date: .abbreviated, time: .shortened))
                        }
                        Button("Open menu camera", systemImage: "camera") { Task { await model.openCamera() } }
                    }
                    Section("Truck") {
                        LabeledContent("Name", value: profile.settings.name)
                        LabeledContent("Cuisine", value: profile.settings.cuisine)
                        Button("Operating settings") { model.showSettings = true }
                        Button("Sign out", role: .destructive) { Task { await model.signOut() } }
                    }
                } else {
                    Section("Truck registration") {
                        if let registration = model.registration {
                            LabeledContent("Truck", value: registration.name)
                            LabeledContent("Status", value: registration.status.rawValue.capitalized)
                        }
                        switch model.registration?.status {
                        case .approved:
                            Text("Set your weekly hours and operating area before sharing from the app.")
                            Button("Set up operating hours and area") { model.showSettings = true }
                        case .pending:
                            Text("Your truck is awaiting review.")
                            Button("Check status") { Task { await model.refreshRegistration() } }.disabled(model.busy)
                        case .rejected:
                            Text("This registration was declined.")
                        case nil:
                            Text("Use your Rolling Local registration link to apply for a truck listing.")
                            Button("Check status") { Task { await model.refreshRegistration() } }.disabled(model.busy)
                        }
                        Button("Sign out", role: .destructive) { Task { await model.signOut() } }
                    }
                }
                if let message = model.message {
                    Section { Text(message).accessibilityIdentifier("operator-status") }
                }
            }
            .navigationTitle(model.registration?.name ?? "Rolling Local")
        }
        .sheet(isPresented: $model.showSettings) {
            SettingsForm(model: model, settings: model.editableSettings)
        }
        .fullScreenCover(isPresented: $model.showCamera) {
            MenuCamera { photo in
                model.showCamera = false
                if let photo {
                    Task { @MainActor in
                        model.draft = CameraDraft(image: photo)
                    }
                }
            }.ignoresSafeArea()
        }
        .sheet(item: $model.draft) { draft in
            NavigationStack {
                ScrollView {
                    Image(uiImage: draft.image).resizable().scaledToFit()
                    Text(draft.capturedAt.formatted(date: .abbreviated, time: .shortened))
                    if let message = model.message { Text(message).foregroundStyle(.secondary) }
                }.padding()
                    .navigationTitle("Menu preview")
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) { Button("Cancel") { model.draft = nil } }
                        ToolbarItem(placement: .confirmationAction) {
                            Button("Publish") { Task { await model.publish(draft) } }.disabled(model.busy)
                        }
                    }
            }
        }
    }
}

struct SettingsForm: View {
    @ObservedObject var model: OperatorModel
    @State var settings: Settings
    @Environment(\.dismiss) private var dismiss

    private let days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]

    var body: some View {
        NavigationStack {
            Form {
                Section("Listing") {
                    TextField("Truck name", text: $settings.name)
                    TextField("Cuisine", text: $settings.cuisine)
                }
                Section("Weekly hours") {
                    TextField("Time zone (America/Phoenix)", text: $settings.timezone)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                    TextField("Opens (HH:mm)", text: $settings.hours.opens)
                    TextField("Closes (HH:mm)", text: $settings.hours.closes)
                    ForEach(1...7, id: \.self) { day in
                        Toggle(days[day - 1], isOn: Binding(get: { settings.hours.weekdays.contains(day) }, set: { enabled in
                            settings.hours.weekdays.removeAll { $0 == day }
                            if enabled { settings.hours.weekdays.append(day) }
                        }))
                    }
                }
                Section("Normal operating circle") {
                    CircleFields(area: $settings.area)
                }
                Section("Temporary event") {
                    Toggle("Use event circle", isOn: Binding(get: { settings.event != nil }, set: { enabled in
                        settings.event = enabled ? EventArea(area: settings.area, expiresAt: .now.addingTimeInterval(7200)) : nil
                    }))
                    if let event = Binding($settings.event) {
                        CircleFields(area: event.area)
                        DatePicker("End", selection: event.expiresAt, in: Date()...)
                    }
                }
                if let message = model.message { Section { Text(message) } }
            }
            .navigationTitle("Truck settings")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        var submitted = settings
                        if submitted.hours.opens.count == 5 { submitted.hours.opens += ":00" }
                        if submitted.hours.closes.count == 5 { submitted.hours.closes += ":00" }
                        if submitted.event.map({ $0.expiresAt <= Date() }) == true { submitted.event = nil }
                        Task { await model.save(submitted) }
                    }.disabled(model.busy)
                }
            }
        }
    }
}

private struct CircleFields: View {
    @Binding var area: Circle

    var body: some View {
        TextField("Center latitude", value: $area.center.latitude, format: .number).keyboardType(.numbersAndPunctuation)
        TextField("Center longitude", value: $area.center.longitude, format: .number).keyboardType(.numbersAndPunctuation)
        TextField("Radius in meters", value: $area.radiusMeters, format: .number).keyboardType(.decimalPad)
    }
}

private struct MenuCamera: UIViewControllerRepresentable {
    var complete: (UIImage?) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(complete: complete) }

    func makeUIViewController(context: Context) -> UIImagePickerController {
        let controller = UIImagePickerController()
        controller.sourceType = .camera
        controller.delegate = context.coordinator
        return controller
    }

    func updateUIViewController(_ uiViewController: UIImagePickerController, context: Context) {}

    final class Coordinator: NSObject, UINavigationControllerDelegate, UIImagePickerControllerDelegate {
        let complete: (UIImage?) -> Void
        init(complete: @escaping (UIImage?) -> Void) { self.complete = complete }

        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) { complete(nil) }

        func imagePickerController(_ picker: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
            complete(info[.originalImage] as? UIImage)
        }
    }
}
