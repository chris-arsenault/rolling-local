import SwiftUI

struct MenuItemsView: View {
    @ObservedObject var model: OperatorModel
    let menuID: UUID
    @State private var result: MenuResults?
    @State private var items: [MenuItem] = []
    @State private var editing = false
    @State private var busy = false
    @State private var message: String?

    var body: some View {
        Form {
            Section("Extraction") {
                if let result {
                    Text(result.extraction?.status.capitalized ?? "Photo only")
                    if let failed = result.extraction, failed.status == "failed" {
                        Text("The photo is published. You can enter its items here.")
                        if !editing {
                            Button("Retry extraction") {
                                Task { await retry(failed) }
                            }.disabled(busy)
                        }
                    }
                }
                if !editing {
                    Button("Refresh") { Task { await load() } }.disabled(busy)
                }
                if let message { Text(message) }
            }
            if editing {
                ForEach($items) { $item in
                    Section {
                        TextField("Item name", text: $item.name)
                        TextField("Description", text: optionalText($item.description), axis: .vertical)
                        TextField("Price as printed", text: optionalText($item.priceText))
                        DisclosureGroup("Tags (\(item.tagIds.count))") {
                            ForEach(result?.tags ?? []) { tag in
                                Toggle(tag.label, isOn: Binding(get: { item.tagIds.contains(tag.id) }, set: { selected in
                                    item.tagIds.removeAll { $0 == tag.id }
                                    if selected { item.tagIds.append(tag.id) }
                                }))
                            }
                        }
                    }
                }
                .onDelete { items.remove(atOffsets: $0) }
                .onMove { items.move(fromOffsets: $0, toOffset: $1) }
                Button("Add item") { items.append(MenuItem()) }
                Button("Save menu items") { Task { await save() } }
                    .disabled(busy || items.contains { $0.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty })
                Button("Cancel edits") { editing = false }.disabled(busy)
            } else if let result {
                ForEach(result.extraction?.items ?? []) { item in
                    Section(item.name) {
                        if let description = item.description { Text(description) }
                        if let price = item.priceText { Text(price) }
                        let labels = result.tags.filter { item.tagIds.contains($0.id) }.map(\.label)
                        if !labels.isEmpty { Text(labels.joined(separator: ", ")) }
                    }
                }
                Button("Edit menu items") {
                    items = result.extraction?.items ?? []
                    editing = true
                }.disabled(busy)
            }
        }
        .environment(\.editMode, .constant(editing ? .active : .inactive))
        .navigationTitle("Menu items")
        .task(id: menuID) { await load() }
    }

    private func optionalText(_ value: Binding<String?>) -> Binding<String> {
        Binding(get: { value.wrappedValue ?? "" }, set: { value.wrappedValue = $0.isEmpty ? nil : $0 })
    }

    @MainActor private func load() async {
        busy = true
        defer { busy = false }
        do {
            let current = try await model.menuResults()
            guard current.extraction?.menuId == nil || current.extraction?.menuId == menuID else {
                result = nil
                message = "A newer photo is published. Reopen its menu items."
                return
            }
            result = current
            message = nil
        } catch { message = error.localizedDescription }
    }

    @MainActor private func save() async {
        busy = true
        defer { busy = false }
        do {
            result = try await model.saveMenu(MenuCorrection(menuId: menuID, revision: result?.extraction?.revision, items: items))
            editing = false
            message = "Menu items saved."
        } catch let error as ServerError where error.code == 409 {
            message = "The menu changed. Cancel edits and refresh before saving again."
        } catch { message = error.localizedDescription }
    }

    @MainActor private func retry(_ failed: MenuExtraction) async {
        busy = true
        defer { busy = false }
        do {
            result = try await model.retryMenu(failed)
            message = nil
        } catch { message = error.localizedDescription }
    }
}
