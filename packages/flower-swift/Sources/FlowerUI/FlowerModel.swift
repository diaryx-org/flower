//  FlowerModel.swift
//
//  The observable owner of a document, shared across macOS and iOS.
//  `FlowerModel` is a platform-neutral `ObservableObject` that owns the
//  `FlowerDoc` and exposes flower-core's page projection: `FlowerPages`
//  (FlowerPagesUI) renders its frames and drives it back through `PageDriving`.
//
//  One projection, on purpose. Core still offers the tree; the Swift surface
//  does not, because the page view covers both of its jobs — a small document
//  inlines onto one page under a generous inline budget (`setInlineBudget`),
//  and a deep one pays for depth with navigation instead of indentation.
//
//  Usage:
//      @StateObject private var model = try! FlowerModel(
//          source: "title = \"flower\"\n", format: "toml")
//
//      var body: some View { FlowerPages(model: model) }

import SwiftUI
import FlowerFFI

/// The observable owner of a document. Hold it with `@StateObject`; render its
/// pages with `FlowerPages` and call the intent methods from taps and buttons.
public final class FlowerModel: ObservableObject {
    /// The latest **page** frame: the page you are on, the page it came out of,
    /// and the page the cursor would open. Replaced wholesale after every
    /// command.
    @Published public private(set) var pages: PagesView

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
        self.pages = doc.pages()
    }

    /// Set how much of a container's subtree inlines onto its parent's page
    /// rather than drilling: at most `rows` rows per inlined subtree, reaching
    /// at most `depth` ranks of inset.
    ///
    /// The default (6 rows, 1 rank) is the settings-menu rule. Raised past the
    /// document's size, the root page simply *is* the whole document — the
    /// host's call, because the right amount is about the room the pages are
    /// drawn in, not about the document.
    public func setInlineBudget(rows: Int, depth: Int) {
        apply(doc.setInlineBudget(rows: UInt32(rows), depth: UInt32(depth)))
    }

    /// The document root's kind — `"map"`, `"seq"`, or `"scalar"`.
    public var rootKind: String { pages.rootKind }
    /// How many managed (hidden) top-level keys the document carries.
    public var hiddenCount: Int { Int(pages.hiddenCount) }

    // ── host-facing model access ──────────────────────────────────────────────

    public func source() -> String { doc.source() }
    public func markSaved() {
        _ = doc.markSaved()
        apply(doc.pages())
    }
    public var isDirty: Bool { pages.dirty }
    public var status: String { pages.status }

    // ── the page projection ───────────────────────────────────────────────────

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

    /// Claim the page projection. The one surface there is, so this only
    /// refreshes the frame — kept because `FlowerPages` announces itself with it
    /// (``PageDriving/showPages()``), and a second host's model may have more to
    /// do here.
    public func showPages() {
        apply(doc.showPages())
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

    // ── editing ───────────────────────────────────────────────────────────────

    /// Open the scalar `item` for inline editing, seeded with its current text.
    public func beginEdit(_ item: PageItemView) {
        guard item.role == "scalar", editingId != item.id else { return }
        if editingId != nil { commitEdit() }
        apply(doc.pageSelect(id: item.id))
        editBuffer = item.preview
        editingId = item.id
    }

    /// Commit the in-flight edit: parse the buffer by literal shape and splice it
    /// losslessly through fig.
    public func commitEdit() {
        guard let id = editingId else { return }
        editingId = nil
        apply(doc.pageSetValue(id: id, text: editBuffer))
    }

    public func cancelEdit() {
        editingId = nil
    }

    /// Set a boolean scalar directly — the commit behind an inline `Toggle`.
    public func setBool(_ item: PageItemView, _ value: Bool) {
        apply(doc.pageSetValue(id: item.id, text: value ? "true" : "false"))
    }

    /// Commit a term chosen from a controlled vocabulary.
    ///
    /// The same write `setBool` makes, for the same reason: a value picked off a
    /// list has no half-typed state to hold, so it goes straight to the document
    /// rather than through the edit buffer.
    ///
    /// This binding carries no schema of its own — `PageItemView` has no
    /// vocabulary to offer — so nothing here reaches it yet. It exists because
    /// the intent belongs to the protocol rather than to the host that first
    /// needed it, and a host that *does* resolve a schema drives the same view
    /// through the same call.
    public func setChoice(_ item: PageItemView, _ value: String) {
        apply(doc.pageSetValue(id: item.id, text: value))
    }

    /// Delete the mapping entry or sequence item `item` names.
    public func delete(_ item: PageItemView) {
        if editingId == item.id { editingId = nil }
        apply(doc.pageDelete(id: item.id))
    }

    // ── key rename ────────────────────────────────────────────────────────────

    /// Open `item`'s key for renaming. A no-op on a sequence item, which has an
    /// index, not a key.
    public func beginRename(_ item: PageItemView) {
        guard item.canRename, renamingId != item.id else { return }
        apply(doc.pageSelect(id: item.id))
        editingId = nil
        renameBuffer = item.label
        renamingId = item.id
    }

    /// Commit the in-flight key rename.
    public func commitRename() {
        guard let id = renamingId else { return }
        renamingId = nil
        let name = renameBuffer.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return }
        apply(doc.pageRename(id: id, newKey: name))
    }

    public func cancelRename() {
        renamingId = nil
    }

    // ── insert & reorder ──────────────────────────────────────────────────────

    public func moveItemUp(_ item: PageItemView) { apply(doc.pageMoveItemUp(id: item.id)) }
    public func moveItemDown(_ item: PageItemView) { apply(doc.pageMoveItemDown(id: item.id)) }

    /// Add a child to the container `id` names — `""` for the document root, or
    /// `pages.page.focus` for the page you are on — and open the new entry for
    /// editing.
    ///
    /// A page names its own container, so "add to this page" and "add to that row"
    /// are one call with a different id.
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
        // A row on this page: its children are the members inlined under it,
        // when it carries any — the run of deeper insets that follows it, kept
        // to the rank directly below its own. Otherwise the page it opens is
        // not loaded and a plain `new_key` is as good a guess as any.
        guard let at = pages.page.items.firstIndex(where: { $0.id == id }) else { return [] }
        let base = pages.page.items[at].inset
        return Set(
            pages.page.items[(at + 1)...]
                .prefix { $0.inset > base }
                .filter { $0.inset == base + 1 }
                .map(\.label)
        )
    }

    private func freshKey(among existing: Set<String>) -> String {
        if !existing.contains("new_key") { return "new_key" }
        var n = 2
        while existing.contains("new_key\(n)") { n += 1 }
        return "new_key\(n)"
    }

    // ── the one seam to the handle ────────────────────────────────────────────

    private func apply(_ view: PagesView) {
        lastMove = move(from: pages.page, to: view.page)
        pages = view
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
}
