//  PageDriving.swift
//
//  The other half of the seam: what the page view *sends*, as opposed to what it
//  reads (PageProtocols.swift).
//
//  Every method here is an intent, not a mutation. The view never edits a
//  document — it says "the user tapped this row" and the conforming model
//  decides what that means, commits it through whatever editor it owns, and
//  publishes a new frame. That is the same contract `FlowerModel` already had
//  with flower-core; naming it as a protocol only makes it something a second
//  host can satisfy.

import Foundation

/// A model the page view can drive: it publishes frames, and it accepts intents.
///
/// `ObservableObject` because the view observes it — a frame changes on every
/// edit and every navigation, and the editing buffers are two-way bindings the
/// row's text fields write into directly.
public protocol PageDriving: ObservableObject {
    associatedtype Pages: PagesDisplaying

    // ── what the view reads ───────────────────────────────────────────────────

    /// The live frame: the pane you are on, the one behind it, the one ahead.
    var pages: Pages { get }
    /// Which way the focus last moved, for the slide's direction.
    var lastMove: PageMove { get }
    /// The id of the row whose *value* is being edited, if any.
    var editingId: String? { get }
    /// The id of the row whose *key* is being renamed, if any.
    var renamingId: String? { get }
    /// The value being typed. A binding: the row's text field writes here.
    var editBuffer: String { get set }
    /// The key being typed. Likewise.
    var renameBuffer: String { get set }
    /// Whether there is anywhere to go back to.
    var canPageBack: Bool { get }

    // ── what the view sends ───────────────────────────────────────────────────

    /// Switch the model to the page projection. Sent when the view appears.
    func showPages()
    /// Open the container `id` names — the far end of a compressed chain, if it
    /// is one. `""` is the document root.
    func pageOpen(id: String)
    /// The page listing `id`, **without** navigating to it.
    ///
    /// A `NavigationStack` is a pull model: it asks for the screen at an
    /// arbitrary element of its path, including levels the model is not standing
    /// on, so answering by moving the focus would make drawing a screen a
    /// navigation.
    func pageAt(id: String) -> Pages.Page
    /// Pop one level.
    func pageBack()
    /// The row's default action: open a container, or begin editing a scalar.
    func pageActivate(_ item: Pages.Page.Item)

    func beginEdit(_ item: Pages.Page.Item)
    func commitEdit()
    func cancelEdit()

    func beginRename(_ item: Pages.Page.Item)
    func commitRename()
    func cancelRename()

    /// Commit a boolean immediately — a switch has no separate confirm.
    func setBool(_ item: Pages.Page.Item, _ value: Bool)

    /// Commit a term picked from a controlled vocabulary
    /// (``PageItemDisplaying/enumOptions``), immediately.
    ///
    /// Separate from ``beginEdit(_:)``/``commitEdit()`` for the same reason
    /// ``setBool(_:_:)`` is: those two exist to hold a *half-typed* value
    /// somewhere that is not the document, because a key being typed one
    /// character at a time is not a key. Choosing from a list has no half-typed
    /// state to hold — the value is legal the instant it is chosen — so routing
    /// it through the edit buffer would add a confirm step to an interaction
    /// that has already confirmed itself.
    func setChoice(_ item: Pages.Page.Item, _ value: String)
    func delete(_ item: Pages.Page.Item)

    /// Whether `item` can take a new child (it is a container).
    func canAddChild(_ item: Pages.Page.Item) -> Bool
    /// Append a key or item to the container `id` names.
    func pageAddChild(id: String)

    func moveItemUp(_ item: Pages.Page.Item)
    func moveItemDown(_ item: Pages.Page.Item)

    // ── adding what a schema declares, for a host that has one ────────────────
    //
    // `pageAddChild(id:)` above is the schemaless half of adding: a placeholder
    // key appears and the reader renames it. A host with a schema knows more —
    // which declared fields this document does not yet carry — and without an
    // offer those fields are unreachable, since rows come from the document and
    // a field with no value has no row. All three are defaulted to "no offer",
    // so a host that says nothing keeps exactly the affordances it had.

    /// Whether the page listing container `id` should offer to add a child at
    /// the bottom of its rows.
    ///
    /// The row-level ``canAddChild(_:)`` cannot answer this: it takes an item,
    /// and the page's own container is never one of its items. Default `false`,
    /// which keeps the add affordance where it always was — the context menu —
    /// for a host that has not decided the page should offer more.
    func canAddChild(pageId: String) -> Bool

    /// The fields a schema declares that container `id` does not yet hold —
    /// what the add menu offers ahead of a custom key. Default: none.
    func addableChildren(of id: String) -> [AddableChild]

    /// Add the declared field `key` to container `id`, holding `value`.
    ///
    /// Separate from ``pageAddChild(id:)`` because a declared field arrives
    /// knowing its name — and sometimes its value: a closed vocabulary rejects
    /// an empty placeholder outright, so the term is chosen in the menu and the
    /// field lands legal (``AddableChild/terms``). A host that offers
    /// ``addableChildren(of:)`` implements this too; the default forwards to
    /// ``pageAddChild(id:)``, which at least adds *something* visible rather
    /// than swallowing the tap.
    func pageAddChild(id: String, key: String, value: String)
}

public extension PageDriving {
    func canAddChild(pageId: String) -> Bool { false }
    func addableChildren(of id: String) -> [AddableChild] { [] }
    func pageAddChild(id: String, key: String, value: String) { pageAddChild(id: id) }
}

/// One field a schema declares and the document does not yet carry — an entry in
/// the page's add menu, described well enough to be offered by name.
///
/// The presentation half (`title`, `icon`, `tint`, `description`) is the same
/// vocabulary ``PageItemDisplaying`` carries for rows that exist, because the
/// offer *is* the row it would become: a field named "Audience" with an eye on
/// its row should be offered as "Audience", not as `audience`.
public struct AddableChild: Identifiable {
    /// The key the field is stored under — what adding actually writes.
    public let key: String
    /// The schema's name for it; `nil` falls back to prettifying the key.
    public let title: String?
    /// The value kind the field takes (`str`, `bool`, …), for the offer's icon.
    /// `nil` when the schema does not say.
    public let kind: String?
    /// A semantic icon name, as ``PageItemDisplaying/icon``.
    public let icon: String?
    /// A semantic tint name, as ``PageItemDisplaying/tint``.
    public let tint: String?
    /// The schema's help text, as ``PageItemDisplaying/description``.
    public let description: String?
    /// The terms a controlled field must arrive holding one of. Non-empty turns
    /// the offer into a submenu of terms, because an empty placeholder is not a
    /// legal value of a closed vocabulary; empty means the field starts blank.
    public let terms: [String]

    public var id: String { key }

    public init(
        key: String,
        title: String? = nil,
        kind: String? = nil,
        icon: String? = nil,
        tint: String? = nil,
        description: String? = nil,
        terms: [String] = []
    ) {
        self.key = key
        self.title = title
        self.kind = kind
        self.icon = icon
        self.tint = tint
        self.description = description
        self.terms = terms
    }
}
