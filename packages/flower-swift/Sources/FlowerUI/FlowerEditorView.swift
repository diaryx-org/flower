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

    let doc: FlowerDoc

    /// Parse `source` as `format` (`"toml"`, `"json"`, `"yaml"`, `"zon"`, `"fig"`, …).
    public init(source: String, format: String) throws {
        let doc = try FlowerDoc(source: source, format: format)
        self.doc = doc
        self.state = doc.view()
    }

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

/// One row of the tree: indentation, a disclosure/scalar glyph, the key, and the
/// value — swapped for a text field while this row is being edited.
private struct FlowerRow: View {
    let row: RowView
    @ObservedObject var model: FlowerModel
    let theme: FlowerTheme
    @FocusState private var focused: Bool

    private var isSelected: Bool { model.selectedRow?.id == row.id }
    private var isEditing: Bool { model.editingId == row.id }

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

            Text(row.label)
                .font(theme.labelFont)
                .foregroundStyle(.primary)

            if row.isContainer {
                Text(row.preview)
                    .font(theme.valueFont)
                    .foregroundStyle(.tertiary)
                Spacer(minLength: 0)
            } else {
                Text("=").foregroundStyle(.tertiary)
                if isEditing {
                    TextField("value", text: $model.editBuffer)
                        .font(theme.valueFont)
                        .textFieldStyle(.plain)
                        .focused($focused)
                        .onSubmit { model.commitEdit() }
                        #if os(macOS)
                        .onExitCommand { model.cancelEdit() }
                        #endif
                        .onAppear { focused = true }
                } else {
                    Text(row.preview)
                        .font(theme.valueFont)
                        .foregroundStyle(theme.color(forKind: row.kind))
                }
                Spacer(minLength: 0)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture { model.activate(row) }
        .padding(.vertical, 1)
        .background(
            RoundedRectangle(cornerRadius: 5)
                .fill(isSelected ? Color.accentColor.opacity(0.15) : Color.clear)
        )
        .contextMenu {
            if !row.isContainer {
                Button("Edit") { model.beginEdit(row) }
            }
            Button("Delete", role: .destructive) { model.delete(row) }
        }
    }
}
