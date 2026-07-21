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
