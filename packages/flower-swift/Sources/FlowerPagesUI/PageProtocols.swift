//  PageProtocols.swift
//
//  What `FlowerPages` needs of a document, stated as protocols instead of as
//  concrete UniFFI records.
//
//  This is the Swift half of a split flower already made in Rust. There, the
//  projection from a `Model` to a renderable frame is generic over the backend,
//  so an embedder editing something other than a blob of config bytes reuses it;
//  only the UniFFI *handle* stays nailed to one backend, because a UniFFI object
//  cannot be generic. The same argument applies one layer up: the page view's
//  two-pane/stack split, its direction-aware slide, and its reconciliation of a
//  `NavigationStack` path against the model's focus are worth exactly as much to
//  a host whose records come out of a different binding, and none of it is
//  specific to the one this package generates.
//
//  So the views are written against these, and the generated records satisfy
//  them as they are — `extension PageItemView: PageItemDisplaying {}`, no
//  members needed, because the protocol was written from their shape. A second
//  host conforms its own records the same way and shares the layout rather than
//  a copy of it.
//
//  ## No styling here
//
//  Deliberately: a host's chrome is its own. These carry what the document *is*
//  — names, kinds, counts, what a row opens — and never how it should look. A
//  host that wants a different appearance writes different views over the same
//  protocols, or passes its own `FlowerTheme`; nothing here has an opinion.

import Foundation

/// One line of a page: a scalar to edit, a container to open, or the header of a
/// group inlined into this page.
///
/// `Identifiable` by the dotted fig path every flower node is named by, so it can
/// key a `ForEach` directly, and so every intent on ``PageDriving`` takes the
/// same string the document does.
public protocol PageItemDisplaying: Identifiable where ID == String {
    /// The item's dotted fig path — its identity, and what every op takes.
    var id: String { get }
    /// The mapping key, or `[i]` for a sequence item.
    var label: String { get }
    /// A readable stand-in for a sequence item's index. Shown *beside* the label,
    /// never instead of it: the index is what the path addresses.
    var title: String? { get }
    /// The value kind as a renderer class id: `str`, `int`, `bool`, `map`, …
    var kind: String { get }
    /// What activating this does: `scalar`, `drill`, or `group`.
    var role: String { get }
    /// A one-line rendering of the value, and the seed text an editor opens with.
    var preview: String { get }
    /// A container's whole contents in flow form, when they fit.
    var summary: String? { get }
    /// How many children a container holds; 0 for a scalar.
    var count: UInt32 { get }
    /// 0 for a direct child of the page, 1 for a member of an inlined group.
    var inset: UInt32 { get }
    /// The names this row shows, outermost first. Several for a compressed drill
    /// — `["exports", "journal"]`, one row standing for a chain of containers
    /// that hold only each other.
    var chain: [String] { get }
    /// Whether this is a mapping entry, whose key can be renamed.
    var canRename: Bool { get }
    /// Whether this belongs below the fields a reader came to edit.
    var demoted: Bool { get }

    // ── what the schema says, for a host that has one ─────────────────────────
    //
    // flower-core already carries a `Schema` — `Constraint::Enum { values,
    // closed }` and the rest — so a host that resolves it knows things about a
    // scalar that the scalar's own *value* does not say: that only certain terms
    // are legal, and that some fields are not the reader's to type at all. A
    // renderer that cannot see any of it has one rendering for every scalar, a
    // text box, which is how a closed vocabulary ends up offered as free text
    // and a maintained stamp ends up looking editable.
    //
    // Defaulted, every one of them, because plenty of documents have no schema
    // behind them: a conformer that says nothing gets exactly the unconstrained
    // rendering that was the only one available before, so an existing empty
    // conformance keeps compiling *and* keeps meaning what it meant.

    /// The terms this field may take, when a schema confines it to a vocabulary.
    /// Empty — the default — means free text.
    var enumOptions: [String] { get }

    /// Whether ``enumOptions`` is the whole of what is legal.
    ///
    /// The distinction is not decoration. A **closed** vocabulary rejects
    /// anything else, so a renderer can offer the terms and nothing else and
    /// lose nothing. An **open** one is a set of suggestions over a wider legal
    /// space — the answers most documents give, with an unlisted value still
    /// perfectly valid — so a renderer that offered only the list would be
    /// hiding legal values, which is a worse failure than offering free text
    /// was.
    var isClosedEnum: Bool { get }

    /// Whether this value is maintained by something other than the person
    /// looking at it: a hash recomputed on every write, an identity, a relation
    /// some other surface owns.
    ///
    /// Readable, never typeable. Offering an editor here promises an edit that
    /// the next save silently reverses, which is worse than showing no control
    /// at all — the reader believes the document says something it does not.
    var isReadonly: Bool { get }

    // ── what the schema calls it ──────────────────────────────────────────────
    //
    // The three above say what a field *may hold*. These three say what it is,
    // and they exist because the alternative was guessing: with nothing but a
    // key and a kind to go on, a renderer infers a name by title-casing the key
    // and an icon by looking for substrings in it. That inference is a decent
    // floor for a document nobody has described, and it is strictly worse than
    // an answer for one somebody has — a schema that went to the trouble of
    // saying `spec` is called "Config format version" and carries a lock should
    // not be overruled by a table that reads the letters of the key.
    //
    // Hints, not instructions. Nothing here is required to render, and a host
    // that wants its own vocabulary of symbols maps these names itself — they
    // are the schema's semantics (`lock`, `danger`), never a host's assets.

    /// The schema's own name for this field — "Config format version" where the
    /// key is `spec`.
    ///
    /// `nil` when no schema named it, and a renderer falls back to prettifying
    /// the key. It never replaces ``label``: renames, paths and every op still
    /// take the key the document actually has, so a title is what the row
    /// *reads* as and the key remains what it *is*.
    var displayTitle: String? { get }

    /// A semantic icon name, from the schema's vocabulary rather than a host's
    /// asset catalogue: `link`, `enum`, `toggle`, `lock`, `globe`, `clock`,
    /// `tag`, `text` — or any other string, which a schema is free to invent
    /// and a renderer that does not know it falls back on.
    var icon: String? { get }

    /// A semantic tint name: `accent`, `neutral`, `positive`, `warning`,
    /// `danger`.
    ///
    /// A *meaning*, not a colour — "this one is dangerous", not "this one is
    /// red" — so a host maps it to whatever its own theme says danger looks
    /// like, in either appearance.
    var tint: String? { get }

    /// Help text for the field: the sentence a schema author would put beside
    /// it, shown as a subtitle and as a tooltip where a host has them.
    var description: String? { get }
}

public extension PageItemDisplaying {
    var enumOptions: [String] { [] }
    var isClosedEnum: Bool { false }
    var isReadonly: Bool { false }
    var displayTitle: String? { nil }
    var icon: String? { nil }
    var tint: String? { nil }
    var description: String? { nil }
}

/// One step of a breadcrumb: what it names, and the id that opens it.
public protocol CrumbDisplaying: Identifiable where ID == String {
    var id: String { get }
    var label: String { get }
}

/// One container's children, ready to render as a pane.
public protocol PageDisplaying {
    associatedtype Item: PageItemDisplaying
    associatedtype Crumb: CrumbDisplaying

    /// The dotted path of the container being listed; `""` is the document root.
    var focus: String { get }
    /// The trail from the root down to `focus`. Empty at the root, whose name is
    /// the host's to choose.
    var crumbs: [Crumb] { get }
    var items: [Item] { get }
    /// The selected item, or `nil` for a pane that does not hold the cursor.
    var selected: UInt32? { get }
    /// Whether this whole pane sits under a demoted key.
    var demoted: Bool { get }
}

/// A whole frame of the page view: the pane you are on, the one it came out of,
/// and the one it would open.
public protocol PagesDisplaying {
    associatedtype Page: PageDisplaying

    var page: Page { get }
    /// One level out — the left pane. `nil` at the root.
    var parent: Page? { get }
    /// What the selection would open, so a split layout is never half empty.
    var peek: Page? { get }
    /// Whether a two-pane layout is worth drawing at all.
    var twoPane: Bool { get }
}

/// Which way the focus last moved, which is the only thing that distinguishes
/// "one level deeper" from "one level out" once the contents have changed.
///
/// Decided where the move happens rather than by the view comparing frames, so
/// the direction and the frame it describes arrive in the same update. A view
/// that worked it out afterwards would animate each navigation the way the
/// *last* one went.
public enum PageMove {
    /// Deeper into the trail.
    case push
    /// Back out along it.
    case pop
    /// Somewhere else entirely — a breadcrumb tap, a view switch, a delete that
    /// popped the page you were standing on. No direction, so it fades.
    case jump
}
