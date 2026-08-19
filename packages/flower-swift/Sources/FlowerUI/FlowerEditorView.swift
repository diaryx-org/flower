//  FlowerEditorView.swift
//
//  The SwiftUI face of the structural editor, shared across macOS and iOS.
//  `FlowerModel` is a platform-neutral `ObservableObject` that owns the
//  `FlowerDoc` and exposes flower-core's navigation + edit commands. `FlowerEditor`
//  is the tree view that renders the model's visible rows and drives it back;
//  `FlowerPages` (FlowerPageView.swift) renders the other projection — one
//  container at a time — off the same model.
//
//  The model holds both projections at once, because core does: `state` is the
//  tree frame, `pages` is the page frame, and every command refreshes both, so
//  switching surfaces never shows a stale one. `projection` says which one the
//  document is being driven through — the seam an edit committed from an inline
//  field reads to know which way to send it.
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

/// Which of core's two projections a document is being driven through — the tree
/// (`FlowerEditor`) or the page view (`FlowerPages`). An inline edit reads it on
/// commit, since a node id means the same thing in both and only the route back
/// into core differs.
public enum FlowerProjection {
    case tree
    case pages
}

/// Which way the last page navigation went, for a frontend that animates along
/// it: a `push` went one level deeper, a `pop` came one level out, and a `jump`
/// is everything else — a sibling opened from the pane beside you, a breadcrumb
/// tapped several levels up, a cursor carried in from the tree.
///
/// It is decided where the move happens rather than by the view comparing frames,
/// so the direction and the frame it describes arrive in the same update. A view
/// that worked it out afterwards would animate each navigation the way the *last*
/// one went.
public enum PageMove {
    case push
    case pop
    case jump
}

/// The observable owner of a document. Hold it with `@StateObject`; render its
/// `state.rows` and call the command methods from taps, buttons, or keys.
public final class FlowerModel: ObservableObject {
    /// The latest rendered frame — the visible rows, selection, dirty, and status.
    /// Replaced wholesale after every command.
    @Published public private(set) var state: DocView

    /// The latest **page** frame: the page you are on, the page it came out of,
    /// and the page the cursor would open. Refreshed by every command, so a host
    /// can switch surfaces without a reload.
    @Published public private(set) var pages: PagesView

    /// Which projection the document is being driven through. `FlowerEditor` sets
    /// it to `.tree` and `FlowerPages` to `.pages`; a commit reads it to know
    /// which of the two an inline edit belongs to.
    @Published public private(set) var projection: FlowerProjection = .tree

    /// Which way the page view last moved — see ``PageMove``.
    @Published public private(set) var lastMove: PageMove = .jump

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
        self.pages = doc.pages()
    }

    /// The document root's kind — `"map"`, `"seq"`, or `"scalar"`.
    public var rootKind: String { state.rootKind }
    /// How many managed (hidden) top-level keys the document carries.
    public var hiddenCount: Int { Int(state.hiddenCount) }

    // ── host-facing model access ──────────────────────────────────────────────

    public func source() -> String { doc.source() }
    public func markSaved() { apply(doc.markSaved()) }
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
        apply(doc.select(index: i))
    }

    public func toggle(_ row: RowView) {
        guard let i = index(of: row.id) else { return }
        apply(doc.toggle(index: i))
    }

    public func moveUp() { apply(doc.moveUp()) }
    public func moveDown() { apply(doc.moveDown()) }
    public func expandOrEnter() { apply(doc.expandOrEnter()) }
    public func collapseOrLeave() { apply(doc.collapseOrLeave()) }

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
    ///
    /// The buffer belongs to a *node*, whichever surface opened it, so the commit
    /// routes by ``projection`` — the tree speaks row indices and
    /// the page view speaks ids, and a node open for editing in one is not
    /// necessarily even visible in the other.
    public func commitEdit() {
        guard let id = editingId else { return }
        editingId = nil
        switch projection {
        case .pages:
            apply(doc.pageSetValue(id: id, text: editBuffer))
        case .tree:
            guard let i = index(of: id) else { return }
            apply(doc.setValue(index: i, text: editBuffer))
        }
    }

    public func cancelEdit() {
        editingId = nil
    }

    /// Delete the mapping entry or sequence item.
    public func delete(_ row: RowView) {
        guard let i = index(of: row.id) else { return }
        if editingId == row.id { editingId = nil }
        apply(doc.delete(index: i))
    }

    /// Set a boolean scalar directly — the commit behind an inline `Toggle`.
    public func setBool(_ row: RowView, _ value: Bool) {
        guard let i = index(of: row.id) else { return }
        apply(doc.setValue(index: i, text: value ? "true" : "false"))
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
            apply(doc.appendItem(index: i, text: ""))
            if let created = state.rows.first(where: { $0.id == prefix + String(count) }) {
                beginEdit(created)
            }
        } else {
            let key = freshKey(under: row)
            apply(doc.insertKey(index: i, key: key, text: ""))
            if let created = state.rows.first(where: { $0.id == prefix + key }) {
                beginEdit(created)
            }
        }
    }

    /// Whether `row` can be reordered (anything but the document root).
    public func canReorder(_ row: RowView) -> Bool { !row.id.isEmpty }

    public func moveRowUp(_ row: RowView) {
        guard let i = index(of: row.id) else { return }
        apply(doc.moveRowUp(index: i))
    }

    public func moveRowDown(_ row: RowView) {
        guard let i = index(of: row.id) else { return }
        apply(doc.moveRowDown(index: i))
    }

    /// Add a top-level entry at the document root: a fresh `new_key` for a mapping
    /// root, or an appended item for a sequence root — then open it for editing.
    /// The root has no row to select, so this is separate from `addChild`.
    public func addRootChild() {
        if rootKind == "seq" {
            let count = state.rows.filter { $0.depth == 0 }.count
            apply(doc.appendRootItem(text: ""))
            if let created = state.rows.first(where: { $0.id == String(count) }) {
                beginEdit(created)
            }
        } else if rootKind == "map" {
            let key = freshRootKey()
            apply(doc.insertRootKey(key: key, text: ""))
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

    /// Commit the in-flight key rename, routed the same way as an edit.
    public func commitRename() {
        guard let id = renamingId else { return }
        renamingId = nil
        let name = renameBuffer.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return }
        switch projection {
        case .pages:
            apply(doc.pageRename(id: id, newKey: name))
        case .tree:
            guard let i = index(of: id) else { return }
            apply(doc.renameKey(index: i, newKey: name))
        }
    }

    public func cancelRename() {
        renamingId = nil
    }

    // ── the page projection ───────────────────────────────────────────────────
    //
    // The page surface drives the same document by the same node ids the tree
    // uses, so `editingId` / `editBuffer` are shared: an edit belongs to a node,
    // not to the list it was started from. Only the commit differs, and
    // `projection` is what tells it which way to go.

    /// The page currently being listed.
    public var page: PageView { pages.page }

    /// The selected item on that page, if it has any.
    public var selectedItem: PageItemView? {
        guard let i = pages.page.selected, pages.page.items.indices.contains(Int(i)) else {
            return nil
        }
        return pages.page.items[Int(i)]
    }

    /// The page listing what `id` names, without navigating to it — `""` for the
    /// document root.
    ///
    /// For a stack-shaped frontend, whose destination builder is asked to render
    /// levels the model is not standing on. It carries a cursor only for the page
    /// you are actually on.
    public func pageAt(id: String) -> PageView { doc.pageAt(id: id) }

    /// Switch to the page view. The cursor carries across, so the node selected in
    /// the tree is the node selected on the page you land on.
    public func showPages() {
        projection = .pages
        apply(doc.showPages())
    }

    /// Switch back to the tree view, carrying the cursor the same way.
    public func showTree() {
        projection = .tree
        apply(doc.showTree())
    }

    /// Put the cursor on `item` — a tap on a row, in any pane.
    public func pageSelect(_ item: PageItemView) {
        if editingId != nil, editingId != item.id { commitEdit() }
        apply(doc.pageSelect(id: item.id))
    }

    /// Open what `id` names as a page: a drill row, a row in the pane you came out
    /// of, or a breadcrumb. `""` is the document root.
    public func pageOpen(id: String) {
        if editingId != nil { commitEdit() }
        apply(doc.pageOpen(id: id))
    }

    /// A tap on a page row: a drill opens, a scalar opens for editing, and a group
    /// header — whose members are already listed under it — just takes the cursor.
    public func pageActivate(_ item: PageItemView) {
        switch item.role {
        case "drill": pageOpen(id: item.id)
        case "scalar": beginEdit(item)
        default: pageSelect(item)
        }
    }

    /// Pop back to the page this one was opened from.
    public func pageBack() {
        if editingId != nil { commitEdit() }
        apply(doc.pageBack())
    }

    /// Whether there is a page to pop back to.
    public var canPageBack: Bool { !pages.page.focus.isEmpty }

    /// Open the scalar `item` for inline editing, seeded with its current text.
    public func beginEdit(_ item: PageItemView) {
        guard item.role == "scalar", editingId != item.id else { return }
        if editingId != nil { commitEdit() }
        apply(doc.pageSelect(id: item.id))
        editBuffer = item.preview
        editingId = item.id
    }

    /// Set a boolean scalar directly — the commit behind an inline `Toggle`.
    public func setBool(_ item: PageItemView, _ value: Bool) {
        apply(doc.pageSetValue(id: item.id, text: value ? "true" : "false"))
    }

    /// Delete the mapping entry or sequence item `item` names.
    public func delete(_ item: PageItemView) {
        if editingId == item.id { editingId = nil }
        apply(doc.pageDelete(id: item.id))
    }

    /// Open `item`'s key for renaming. A no-op on a sequence item, which has an
    /// index, not a key.
    public func beginRename(_ item: PageItemView) {
        guard item.canRename, renamingId != item.id else { return }
        apply(doc.pageSelect(id: item.id))
        editingId = nil
        renameBuffer = item.label
        renamingId = item.id
    }

    public func moveItemUp(_ item: PageItemView) { apply(doc.pageMoveItemUp(id: item.id)) }
    public func moveItemDown(_ item: PageItemView) { apply(doc.pageMoveItemDown(id: item.id)) }

    /// Add a child to the container `id` names — `""` for the document root, or
    /// `pages.page.focus` for the page you are on — and open the new entry for
    /// editing.
    ///
    /// A page names its own container, so "add to this page" and "add to that row"
    /// are one call with a different id. That is not true of the tree, where the
    /// root has no row and needs a method of its own.
    public func pageAddChild(id: String) {
        let existing = Set(pages.page.items.map(\.id))
        let key = freshKey(among: childLabels(of: id))
        apply(doc.pageAddChild(id: id, key: key, text: ""))
        // Whatever the page gained is the entry to open — the id of an appended
        // sequence item is its index, which we would otherwise have to predict.
        if let created = pages.page.items.first(where: { !existing.contains($0.id) && $0.role == "scalar" }) {
            beginEdit(created)
        }
    }

    /// Whether "add" applies to `item` — it names a container of some kind.
    public func canAddChild(_ item: PageItemView) -> Bool { item.role != "scalar" }

    /// The labels already used by the children of the container `id` names, so a
    /// fresh key doesn't collide.
    private func childLabels(of id: String) -> Set<String> {
        if id == pages.page.focus {
            return Set(pages.page.items.filter { $0.inset == 0 }.map(\.label))
        }
        // A row on this page: its children are the group members inlined under it,
        // when it is one; otherwise the page it opens is not loaded and a plain
        // `new_key` is as good a guess as any.
        guard let at = pages.page.items.firstIndex(where: { $0.id == id }) else { return [] }
        return Set(
            pages.page.items[(at + 1)...].prefix { $0.inset > 0 }.map(\.label)
        )
    }

    private func freshKey(among existing: Set<String>) -> String {
        if !existing.contains("new_key") { return "new_key" }
        var n = 2
        while existing.contains("new_key\(n)") { n += 1 }
        return "new_key\(n)"
    }

    // ── keeping the two frames in step ────────────────────────────────────────
    //
    // Core keeps both projections live off one document, so the model does too:
    // every command replaces the frame it was made from and re-reads the other.
    // The alternative — refreshing lazily on a surface switch — shows a stale
    // page for exactly as long as it takes the user to notice.

    private func apply(_ view: DocView) {
        state = view
        pages = doc.pages()
    }

    private func apply(_ view: PagesView) {
        lastMove = move(from: pages.page, to: view.page)
        pages = view
        state = doc.view()
    }

    /// How the page view got from `old` to `new`, by depth: the trail is the
    /// lineage, so one step longer is a push and one step shorter is a pop.
    /// Everything else — including a move that keeps the depth, like opening a
    /// sibling from the pane beside you — is a jump, which has no direction to
    /// animate along.
    private func move(from old: PageView, to new: PageView) -> PageMove {
        guard old.focus != new.focus else { return lastMove }
        switch new.crumbs.count - old.crumbs.count {
        case 1: return .push
        case -1: return .pop
        default: return .jump
        }
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
