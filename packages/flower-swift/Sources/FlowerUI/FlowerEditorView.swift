//  FlowerEditorView.swift
//
//  The SwiftUI face of the structural editor, shared across macOS and iOS.
//  `FlowerModel` is a platform-neutral `ObservableObject` that owns the
//  `FlowerDoc` and exposes flower-core's navigation + edit commands. `FlowerEditor`
//  is the tree view that renders the model's visible rows and drives it back.
//
//  Usage:
//      @StateObject private var model = try! FlowerModel(
//          source: "title = \"flower\"\n", format: "toml")
//
//      var body: some View { FlowerEditor(model: model) }

import SwiftUI
import FlowerFFI

// A `RowView` already carries a stable `id` (its dotted fig path), so it can key a
// SwiftUI `ForEach`/`List` directly.
extension RowView: @retroactive Identifiable {}

/// The observable owner of a document. Hold it with `@StateObject`; render its
/// `state.rows` and call the command methods from taps, buttons, or keys.
public final class FlowerModel: ObservableObject {
    /// The latest rendered frame — the visible rows, selection, dirty, and status.
    /// Replaced wholesale after every command.
    @Published public private(set) var state: DocView

    /// The row currently open for inline editing (its `id`), or `nil`. Drives which
    /// row swaps its value text for a text field.
    @Published public var editingId: String?
    /// The text field's live buffer while a scalar is being edited.
    @Published public var editBuffer: String = ""

    /// The row whose **key** is being renamed (its `id`), or `nil`.
    @Published public var renamingId: String?
    /// The live buffer while a key is being renamed.
    @Published public var renameBuffer: String = ""

    let doc: FlowerDoc

    /// Parse `source` as `format` (`"toml"`, `"json"`, `"yaml"`, `"zon"`, `"fig"`, …).
    ///
    /// `hiddenKeys` are top-level mapping keys to hide from view while keeping them
    /// in the document (byte-for-byte) — pass the managed-key set for a
    /// prov/diaryx frontmatter, or `[]` for a standalone config. The list is the
    /// caller's to supply (e.g. from a diaryx binding); FlowerUI never names them.
    public init(source: String, format: String, hiddenKeys: [String] = []) throws {
        let doc = try FlowerDoc(source: source, format: format, hiddenKeys: hiddenKeys)
        self.doc = doc
        self.state = doc.view()
    }

    /// The document root's kind — `"map"`, `"seq"`, or `"scalar"`.
    public var rootKind: String { state.rootKind }
    /// How many managed (hidden) top-level keys the document carries.
    public var hiddenCount: Int { Int(state.hiddenCount) }

    // ── host-facing model access ──────────────────────────────────────────────

    public func source() -> String { doc.source() }
    public func markSaved() { state = doc.markSaved() }
    public var isDirty: Bool { state.dirty }
    public var status: String { state.status }

    /// The selected row, if any — for a detail pane or a delete affordance.
    public var selectedRow: RowView? {
        state.rows.indices.contains(Int(state.selected)) ? state.rows[Int(state.selected)] : nil
    }

    // ── selection & navigation ────────────────────────────────────────────────

    private func index(of id: String) -> UInt32? {
        state.rows.firstIndex(where: { $0.id == id }).map(UInt32.init)
    }

    public func select(_ row: RowView) {
        guard let i = index(of: row.id) else { return }
        state = doc.select(index: i)
    }

    public func toggle(_ row: RowView) {
        guard let i = index(of: row.id) else { return }
        state = doc.toggle(index: i)
    }

    public func moveUp() { state = doc.moveUp() }
    public func moveDown() { state = doc.moveDown() }
    public func expandOrEnter() { state = doc.expandOrEnter() }
    public func collapseOrLeave() { state = doc.collapseOrLeave() }

    /// A tap on a row: a container toggles; a scalar opens for editing. Any edit
    /// already in flight on another row is committed first.
    public func activate(_ row: RowView) {
        if let editing = editingId, editing != row.id { commitEdit() }
        if let renaming = renamingId, renaming != row.id { commitRename() }
        if row.isContainer {
            toggle(row)
        } else {
            beginEdit(row)
        }
    }

    // ── editing ───────────────────────────────────────────────────────────────

    /// Open the scalar `row` for inline editing, seeding the field with its
    /// current text. A no-op on a container or a row already being edited.
    public func beginEdit(_ row: RowView) {
        guard !row.isContainer, editingId != row.id else { return }
        select(row)
        editBuffer = row.preview
        editingId = row.id
    }

    /// Commit the in-flight edit: parse the buffer by literal shape and splice it
    /// losslessly through fig.
    public func commitEdit() {
        guard let id = editingId, let i = index(of: id) else { return }
        editingId = nil
        state = doc.setValue(index: i, text: editBuffer)
    }

    public func cancelEdit() {
        editingId = nil
    }

    /// Delete the mapping entry or sequence item.
    public func delete(_ row: RowView) {
        guard let i = index(of: row.id) else { return }
        if editingId == row.id { editingId = nil }
        state = doc.delete(index: i)
    }

    /// Set a boolean scalar directly — the commit behind an inline `Toggle`.
    public func setBool(_ row: RowView, _ value: Bool) {
        guard let i = index(of: row.id) else { return }
        state = doc.setValue(index: i, text: value ? "true" : "false")
    }

    // ── insert & reorder ──────────────────────────────────────────────────────

    /// Whether `row` can hold children — i.e. "Add" applies to it.
    public func canAddChild(_ row: RowView) -> Bool { row.isContainer }

    /// Add a child to the container `row`: a fresh `new_key` for a mapping, or an
    /// appended item for a sequence — then open the new scalar for editing.
    public func addChild(_ row: RowView) {
        guard let i = index(of: row.id), row.isContainer else { return }
        let prefix = row.id.isEmpty ? "" : row.id + "."
        if row.kind == "seq" {
            let count = state.rows.filter { isDirectChild($0, of: row) }.count
            state = doc.appendItem(index: i, text: "")
            if let created = state.rows.first(where: { $0.id == prefix + String(count) }) {
                beginEdit(created)
            }
        } else {
            let key = freshKey(under: row)
            state = doc.insertKey(index: i, key: key, text: "")
            if let created = state.rows.first(where: { $0.id == prefix + key }) {
                beginEdit(created)
            }
        }
    }

    /// Whether `row` can be reordered (anything but the document root).
    public func canReorder(_ row: RowView) -> Bool { !row.id.isEmpty }

    public func moveRowUp(_ row: RowView) {
        guard let i = index(of: row.id) else { return }
        state = doc.moveRowUp(index: i)
    }

    public func moveRowDown(_ row: RowView) {
        guard let i = index(of: row.id) else { return }
        state = doc.moveRowDown(index: i)
    }

    /// Add a top-level entry at the document root: a fresh `new_key` for a mapping
    /// root, or an appended item for a sequence root — then open it for editing.
    /// The root has no row to select, so this is separate from `addChild`.
    public func addRootChild() {
        if rootKind == "seq" {
            let count = state.rows.filter { $0.depth == 0 }.count
            state = doc.appendRootItem(text: "")
            if let created = state.rows.first(where: { $0.id == String(count) }) {
                beginEdit(created)
            }
        } else if rootKind == "map" {
            let key = freshRootKey()
            state = doc.insertRootKey(key: key, text: "")
            if let created = state.rows.first(where: { $0.id == key }) {
                beginEdit(created)
            }
        }
    }

    // ── key rename ────────────────────────────────────────────────────────────

    /// Open the mapping entry `row` for renaming its key. A no-op on a sequence
    /// item (which has an index, not a key).
    public func beginRename(_ row: RowView) {
        guard row.canRename, renamingId != row.id else { return }
        select(row)
        editingId = nil
        renameBuffer = row.label
        renamingId = row.id
    }

    /// Commit the in-flight key rename.
    public func commitRename() {
        guard let id = renamingId, let i = index(of: id) else { return }
        renamingId = nil
        let name = renameBuffer.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return }
        state = doc.renameKey(index: i, newKey: name)
    }

    public func cancelRename() {
        renamingId = nil
    }

    private func isDirectChild(_ r: RowView, of parent: RowView) -> Bool {
        r.depth == parent.depth + 1 && (parent.id.isEmpty || r.id.hasPrefix(parent.id + "."))
    }

    private func freshRootKey() -> String {
        let existing = Set(state.rows.filter { $0.depth == 0 }.map(\.label))
        if !existing.contains("new_key") { return "new_key" }
        var n = 2
        while existing.contains("new_key\(n)") { n += 1 }
        return "new_key\(n)"
    }

    private func freshKey(under row: RowView) -> String {
        let existing = Set(state.rows.filter { isDirectChild($0, of: row) }.map(\.label))
        if !existing.contains("new_key") { return "new_key" }
        var n = 2
        while existing.contains("new_key\(n)") { n += 1 }
        return "new_key\(n)"
    }
}

/// The tree editor surface: a flat, indented list of the document's visible rows.
/// Core already flattens the tree and honours the collapsed set, so this only
/// indents each row by its depth and draws a disclosure twisty for containers.
public struct FlowerEditor: View {
    @ObservedObject private var model: FlowerModel
    private let theme: FlowerTheme

    public init(model: FlowerModel, theme: FlowerTheme = .default) {
        self.model = model
        self.theme = theme
    }

    public var body: some View {
        List {
            ForEach(model.state.rows) { row in
                FlowerRow(row: row, model: model, theme: theme)
                    .listRowInsets(EdgeInsets(top: theme.rowSpacing, leading: 8,
                                              bottom: theme.rowSpacing, trailing: 8))
                    .listRowSeparator(.hidden)
            }
        }
        .listStyle(.plain)
    }
}

/// One row of the tree: indentation, a disclosure/scalar glyph, the key, and a
/// type-aware value editor — a `Toggle` for a bool, a stepped field for a number,
/// a plain field otherwise.
private struct FlowerRow: View {
    let row: RowView
    @ObservedObject var model: FlowerModel
    let theme: FlowerTheme
    @FocusState private var focused: Bool
    @FocusState private var keyFocused: Bool

    private var isSelected: Bool { model.selectedRow?.id == row.id }
    private var isEditing: Bool { model.editingId == row.id }
    private var isRenaming: Bool { model.renamingId == row.id }

    var body: some View {
        HStack(spacing: 6) {
            Color.clear.frame(width: CGFloat(row.depth) * theme.indentWidth, height: 1)

            Button { model.toggle(row) } label: {
                Image(systemName: theme.symbol(isContainer: row.isContainer, expanded: row.expanded))
                    .font(.system(size: row.isContainer ? 11 : 6))
                    .foregroundStyle(row.isContainer ? Color.secondary : Color.secondary.opacity(0.5))
                    .frame(width: 14)
            }
            .buttonStyle(.plain)
            .disabled(!row.isContainer)

            keyView

            if row.isContainer {
                Text(row.preview)
                    .font(theme.valueFont)
                    .foregroundStyle(.tertiary)
                Spacer(minLength: 0)
            } else {
                Text("=").foregroundStyle(.tertiary)
                valueView
                Spacer(minLength: 0)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture { if !(row.kind == "bool") { model.activate(row) } }
        .padding(.vertical, 1)
        .background(
            RoundedRectangle(cornerRadius: 5)
                .fill(isSelected ? Color.accentColor.opacity(0.15) : Color.clear)
        )
        .contextMenu { contextMenu }
    }

    /// The key: a label, or a text field while its key is being renamed.
    @ViewBuilder private var keyView: some View {
        if isRenaming {
            TextField("key", text: $model.renameBuffer)
                .font(theme.labelFont)
                .textFieldStyle(.plain)
                .focused($keyFocused)
                .frame(maxWidth: 160)
                .onSubmit { model.commitRename() }
                #if os(macOS)
                .onExitCommand { model.cancelRename() }
                #endif
                .onAppear { keyFocused = true }
        } else {
            Text(row.label)
                .font(theme.labelFont)
                .foregroundStyle(.primary)
        }
    }

    private var isNumber: Bool { row.kind == "int" || row.kind == "float" }

    /// The type-aware value editor for a scalar row.
    @ViewBuilder private var valueView: some View {
        if row.kind == "bool" {
            // A bool commits immediately — no separate edit mode.
            Toggle("", isOn: Binding(
                get: { row.preview == "true" },
                set: { model.setBool(row, $0) }
            ))
            .labelsHidden()
            .toggleStyle(.switch)
            .scaleEffect(0.7)
            .frame(height: 16)
        } else if isEditing {
            if isNumber { numberEditor } else { editField(keyboardNumeric: false) }
        } else {
            Text(row.preview.isEmpty ? "—" : row.preview)
                .font(theme.valueFont)
                .foregroundStyle(row.preview.isEmpty ? Color.secondary.opacity(0.5)
                                                     : theme.color(forKind: row.kind))
        }
    }

    @ViewBuilder private var numberEditor: some View {
        HStack(spacing: 4) {
            editField(keyboardNumeric: true)
                .frame(maxWidth: 120)
            Button { step(+1) } label: { Image(systemName: "chevron.up") }
                .buttonStyle(.plain).foregroundStyle(.secondary)
            Button { step(-1) } label: { Image(systemName: "chevron.down") }
                .buttonStyle(.plain).foregroundStyle(.secondary)
        }
    }

    private func editField(keyboardNumeric: Bool) -> some View {
        let field = TextField("value", text: $model.editBuffer)
            .font(theme.valueFont)
            .textFieldStyle(.plain)
            .focused($focused)
            .onSubmit { model.commitEdit() }
            .onAppear { focused = true }
        #if os(macOS)
        return field.onExitCommand { model.cancelEdit() }
        #else
        return field.keyboardType(keyboardNumeric ? .numbersAndPunctuation : .default)
        #endif
    }

    /// Bump the numeric edit buffer by `delta`, keeping int/float shape.
    private func step(_ delta: Int) {
        let text = model.editBuffer.trimmingCharacters(in: .whitespaces)
        if let i = Int(text) {
            model.editBuffer = String(i + delta)
        } else if let d = Double(text) {
            model.editBuffer = String(d + Double(delta))
        }
    }

    @ViewBuilder private var contextMenu: some View {
        if !row.isContainer {
            Button("Edit Value") { model.beginEdit(row) }
        }
        if row.canRename {
            Button("Rename Key") { model.beginRename(row) }
        }
        if model.canAddChild(row) {
            Button(row.kind == "seq" ? "Add Item" : "Add Key") { model.addChild(row) }
        }
        if model.canReorder(row) {
            Divider()
            Button("Move Up") { model.moveRowUp(row) }
            Button("Move Down") { model.moveRowDown(row) }
        }
        Divider()
        Button("Delete", role: .destructive) { model.delete(row) }
    }
}
