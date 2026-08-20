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
    func delete(_ item: Pages.Page.Item)

    /// Whether `item` can take a new child (it is a container).
    func canAddChild(_ item: Pages.Page.Item) -> Bool
    /// Append a key or item to the container `id` names.
    func pageAddChild(id: String)

    func moveItemUp(_ item: Pages.Page.Item)
    func moveItemDown(_ item: Pages.Page.Item)
}
