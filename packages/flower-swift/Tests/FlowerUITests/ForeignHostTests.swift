import SwiftUI
import XCTest
@testable import FlowerPagesUI

/// The point of the protocol split, as a test: a host with records of its own —
/// a different UniFFI namespace, a different backend, no `FlowerDoc` anywhere —
/// renders the page editor without touching it.
///
/// Nothing below imports `FlowerFFI` or `FlowerUI`. If `FlowerPages` ever grows
/// a dependency on this package's own binding, or a protocol grows a member only
/// the generated records could satisfy, this file stops compiling — which is the
/// only way to keep a claim like "reusable" honest once the person who made it
/// has moved on.
///
/// It is deliberately the *awkward* case: `count` and `inset` are stored as
/// `Int` here and bridged in the conformance, because a second host's records
/// will not have been generated from flower's own IDL.

// ── a foreign host's records ──────────────────────────────────────────────────

private struct MetaItem: PageItemDisplaying {
    let id: String
    let label: String
    var title: String?
    var kind: String = "str"
    var role: String = "scalar"
    var preview: String = ""
    var summary: String?
    var fieldCount: Int = 0
    var depth: Int = 0
    var chain: [String] = []
    var canRename: Bool = true
    var demoted: Bool = false

    var count: UInt32 { UInt32(fieldCount) }
    var inset: UInt32 { UInt32(depth) }
}

/// A second foreign record, this one with a schema behind it — the case
/// `MetaItem` above deliberately does not cover.
///
/// Two records rather than fields added to one, because both halves need
/// holding: a host with no schema must keep conforming without writing a line
/// (the defaults), and a host with one must be able to say so without flower's
/// binding anywhere in reach.
private struct SchemaItem: PageItemDisplaying {
    let id: String
    let label: String
    var title: String?
    var kind: String = "str"
    var role: String = "scalar"
    var preview: String = ""
    var summary: String?
    var count: UInt32 = 0
    var inset: UInt32 = 0
    var chain: [String] = []
    var canRename: Bool = true
    var demoted: Bool = false

    var enumOptions: [String] = []
    var isClosedEnum: Bool = false
    var isReadonly: Bool = false

    var displayTitle: String?
    var icon: String?
    var tint: String?
    var description: String?
    var linkLabel: String?
}

private struct MetaCrumb: CrumbDisplaying {
    let id: String
    let label: String
}

private struct MetaPage: PageDisplaying {
    var focus: String = ""
    var crumbs: [MetaCrumb] = []
    var items: [MetaItem] = []
    var selected: UInt32?
    var demoted: Bool = false
}

private struct MetaFrame: PagesDisplaying {
    var page: MetaPage
    var parent: MetaPage?
    var peek: MetaPage?
    var twoPane: Bool = false
}

// ── a foreign host's model ────────────────────────────────────────────────────

private final class MetaModel: PageDriving {
    @Published var pages: MetaFrame
    @Published var lastMove: PageMove = .jump
    @Published var editingId: String?
    @Published var renamingId: String?
    @Published var editBuffer: String = ""
    @Published var renameBuffer: String = ""

    /// Every intent the view sent, in order — so a test can assert the view asks
    /// for things rather than doing them.
    private(set) var sent: [String] = []

    init(_ items: [MetaItem]) {
        pages = MetaFrame(page: MetaPage(items: items, selected: 0))
    }

    var canPageBack: Bool { !pages.page.focus.isEmpty }

    func showPages() { sent.append("showPages") }
    func pageOpen(id: String) { sent.append("open:\(id)") }
    func pageAt(id: String) -> MetaPage { MetaPage(focus: id) }
    func pageBack() { sent.append("back") }
    func pageActivate(_ item: MetaItem) { sent.append("activate:\(item.id)") }
    func beginEdit(_ item: MetaItem) { sent.append("edit:\(item.id)") }
    func commitEdit() { sent.append("commitEdit") }
    func cancelEdit() { sent.append("cancelEdit") }
    func beginRename(_ item: MetaItem) { sent.append("rename:\(item.id)") }
    func commitRename() { sent.append("commitRename") }
    func cancelRename() { sent.append("cancelRename") }
    func setBool(_ item: MetaItem, _ value: Bool) { sent.append("bool:\(item.id)=\(value)") }
    func setChoice(_ item: MetaItem, _ value: String) { sent.append("choice:\(item.id)=\(value)") }
    func delete(_ item: MetaItem) { sent.append("delete:\(item.id)") }
    func canAddChild(_ item: MetaItem) -> Bool { item.role != "scalar" }
    func pageAddChild(id: String) { sent.append("add:\(id)") }
    func moveItemUp(_ item: MetaItem) { sent.append("up:\(item.id)") }
    func moveItemDown(_ item: MetaItem) { sent.append("down:\(item.id)") }
}

/// A host whose schema offers declared fields — the add half `MetaModel` above
/// deliberately leaves defaulted. Conforms directly rather than subclassing:
/// protocol witnesses bind at the conformance, so a subclass "overriding" a
/// defaulted requirement would be the dispatch mistake, not a test of one.
private final class DeclaringModel: PageDriving {
    @Published var pages: MetaFrame
    @Published var lastMove: PageMove = .jump
    @Published var editingId: String?
    @Published var renamingId: String?
    @Published var editBuffer: String = ""
    @Published var renameBuffer: String = ""

    private(set) var sent: [String] = []

    init(_ items: [MetaItem]) {
        pages = MetaFrame(page: MetaPage(items: items))
    }

    var canPageBack: Bool { false }

    func showPages() {}
    func pageOpen(id: String) {}
    func pageAt(id: String) -> MetaPage { MetaPage(focus: id) }
    func pageBack() {}
    func pageActivate(_ item: MetaItem) {}
    func beginEdit(_ item: MetaItem) {}
    func commitEdit() {}
    func cancelEdit() {}
    func beginRename(_ item: MetaItem) {}
    func commitRename() {}
    func cancelRename() {}
    func setBool(_ item: MetaItem, _ value: Bool) {}
    func setChoice(_ item: MetaItem, _ value: String) {}
    func delete(_ item: MetaItem) {}
    func canAddChild(_ item: MetaItem) -> Bool { item.role != "scalar" }
    func pageAddChild(id: String) { sent.append("add:\(id)") }
    func moveItemUp(_ item: MetaItem) {}
    func moveItemDown(_ item: MetaItem) {}

    func canAddChild(pageId: String) -> Bool { pageId.isEmpty }
    func addableChildren(of id: String) -> [AddableChild] {
        guard id.isEmpty else { return [] }
        return [AddableChild(key: "audience", title: "Audience", kind: "str",
                             icon: "enum", description: "Who may read this.",
                             terms: ["public", "private"])]
    }
    func pageAddChild(id: String, key: String, value: String) {
        sent.append("add:\(id):\(key)=\(value)")
    }
}

// ── the test ──────────────────────────────────────────────────────────────────

final class ForeignHostTests: XCTestCase {
    private func sample() -> MetaModel {
        MetaModel([
            MetaItem(id: "title", label: "title", preview: "A Note"),
            MetaItem(id: "audience", label: "audience", kind: "str", preview: "public"),
            MetaItem(id: "exports", label: "exports", kind: "map", role: "drill",
                     summary: "{branches: [master]}", fieldCount: 2,
                     chain: ["exports", "journal"]),
            MetaItem(id: "content_hash", label: "content_hash",
                     preview: "ab12", canRename: false, demoted: true),
        ])
    }

    /// The whole claim in one line: the page editor accepts a model that has
    /// never heard of `FlowerDoc`.
    func testTheEditorRendersAHostWithItsOwnRecords() {
        let model = sample()
        let view = FlowerPages(model: model, rootLabel: "note.md")
        XCTAssertNotNil(view.body)
    }

    /// The protocols carry what the document *is*, and the conformance is where a
    /// host's own storage is bridged — `Int` counts here, `UInt32` at the seam.
    func testAForeignRecordSatisfiesTheProtocolWithoutReshapingItself() {
        let item = sample().pages.page.items[2]
        XCTAssertEqual(item.count, 2)
        XCTAssertEqual(item.inset, 0)
        XCTAssertEqual(item.chain, ["exports", "journal"])
        XCTAssertEqual(item.role, "drill")
    }

    /// Demotion and compression survive the boundary, so a second host gets the
    /// two page behaviours flower added for it rather than the record shapes only.
    func testTheShapesFlowerAddedCrossTheSeamToo() {
        let items = sample().pages.page.items
        XCTAssertEqual(items.filter(\.demoted).map(\.id), ["content_hash"])
        XCTAssertFalse(items[0].demoted)
        // A compressed row names its chain; an ordinary one names itself, and a
        // host that never compresses simply leaves the array to match its label.
        XCTAssertEqual(items[2].chain.count, 2)
    }

    /// The view is a renderer, not an editor: everything it can do to a document
    /// it does by asking. A host that conforms `PageDriving` therefore keeps the
    /// same veto over its own document that `FlowerModel` has over flower's.
    func testTheViewOnlyEverSendsIntents() {
        let model = sample()
        XCTAssertTrue(model.sent.isEmpty)
        model.pageActivate(model.pages.page.items[2])
        XCTAssertEqual(model.sent, ["activate:exports"])
    }

    // ── the schema half ───────────────────────────────────────────────────────

    /// A host with no schema says nothing and still conforms — which is what
    /// keeps the addition from being a breaking change to every existing
    /// embedder. `MetaItem` declares none of the three; the defaults answer.
    func testAHostWithoutASchemaInheritsTheUnconstrainedRendering() {
        let item = sample().pages.page.items[1]
        XCTAssertEqual(item.enumOptions, [])
        XCTAssertFalse(item.isClosedEnum)
        XCTAssertFalse(item.isReadonly)
    }

    /// ...and a host that *has* one is read through the protocol rather than
    /// through its own type, which is the only version of this that the view
    /// actually benefits from — `PageRow` sees `some PageItemDisplaying`.
    func testAHostWithASchemaIsReadThroughTheProtocol() {
        func vocabulary(of item: some PageItemDisplaying) -> ([String], Bool) {
            (item.enumOptions, item.isClosedEnum)
        }
        let closed = SchemaItem(id: "metadata_format", label: "metadata_format",
                                preview: "yaml",
                                enumOptions: ["yaml", "toml", "json"],
                                isClosedEnum: true)
        XCTAssertEqual(vocabulary(of: closed).0, ["yaml", "toml", "json"])
        XCTAssertTrue(vocabulary(of: closed).1)

        // The default is not sticky: a stored property wins over the extension,
        // even reached generically. This is the assertion that would catch the
        // dispatch mistake — a requirement declared only in the extension would
        // bind statically and hand back `[]` here.
        let none = SchemaItem(id: "title", label: "title", preview: "A Note")
        XCTAssertEqual(vocabulary(of: none).0, [])
    }

    /// An open vocabulary is a set of suggestions, not a fence, and the flag is
    /// the whole of what tells the two apart at the seam.
    func testAnOpenVocabularyIsDistinguishableFromAClosedOne() {
        let open = SchemaItem(id: "content_format", label: "content_format",
                              preview: "markdown",
                              enumOptions: ["markdown", "html"],
                              isClosedEnum: false)
        XCTAssertFalse(open.isClosedEnum)
        XCTAssertFalse(open.enumOptions.isEmpty)
    }

    /// Picking a term is its own intent, so a host keeps the same veto over a
    /// chosen value that it has over a typed one.
    func testChoosingATermIsSentAsAnIntent() {
        let model = sample()
        model.setChoice(model.pages.page.items[1], "private")
        XCTAssertEqual(model.sent, ["choice:audience=private"])
    }

    // ── what the schema calls it ──────────────────────────────────────────────

    /// The same bargain the vocabulary half struck: a host that never heard of
    /// presentation keeps conforming, and gets the inference it always got.
    func testAHostThatNamesNothingStillConforms() {
        let item = sample().pages.page.items[0]
        XCTAssertNil(item.displayTitle)
        XCTAssertNil(item.icon)
        XCTAssertNil(item.tint)
        XCTAssertNil(item.description)
    }

    /// ...and a host that *has* a schema is read through the protocol, so the
    /// row renders the declared name rather than a title-cased key.
    func testASchemasNameAndSymbolCrossTheSeam() {
        func presentation(of item: some PageItemDisplaying) -> (String?, String?, String?) {
            (item.displayTitle, item.icon, item.tint)
        }
        let spec = SchemaItem(id: "spec", label: "spec", preview: "1",
                              isReadonly: true,
                              displayTitle: "Config format version",
                              icon: "lock",
                              tint: "neutral")
        XCTAssertEqual(presentation(of: spec).0, "Config format version")
        XCTAssertEqual(presentation(of: spec).1, "lock")
        XCTAssertEqual(presentation(of: spec).2, "neutral")

        // Same dispatch trap as the vocabulary half: a stored property must win
        // over the extension's default even when reached generically.
        let plain = SchemaItem(id: "title", label: "title", preview: "A Note")
        XCTAssertNil(presentation(of: plain).0)
    }

    // ── the palette resolves schema first, inference second ───────────────────

    /// The point of the whole layer: what a schema said beats what a key looks
    /// like. `spec` infers nothing in particular; declared, it is a lock.
    func testADeclaredIconBeatsTheInferenceFromTheKey() {
        let inferred = FlowerPalette.icon(label: "spec", kind: "int")
        let declared = FlowerPalette.icon(label: "spec", kind: "int", icon: "lock")
        XCTAssertEqual(inferred.symbol, FlowerPalette.inferredIcon(label: "spec", kind: "int").symbol)
        XCTAssertEqual(declared.symbol, FlowerPalette.symbol(forSemanticIcon: "lock"))
        XCTAssertNotEqual(declared.symbol, inferred.symbol)
    }

    /// The two halves fall back independently: a schema saying only "this one is
    /// dangerous" is taken at its word about the colour and left to the
    /// inference about the symbol, rather than ignored for being incomplete.
    func testASchemaMaySayTheColourWithoutSayingTheSymbol() {
        let inferred = FlowerPalette.inferredIcon(label: "recycle_bin", kind: "bool")
        let tinted = FlowerPalette.icon(label: "recycle_bin", kind: "bool", tint: "danger")
        XCTAssertEqual(tinted.symbol, inferred.symbol)
        XCTAssertEqual(tinted.color, FlowerPalette.color(forTint: "danger"))
        XCTAssertNotEqual(tinted.color, inferred.color)
    }

    /// An unknown name is a schema talking to some other frontend, not a schema
    /// making a mistake — so it falls back rather than rendering a hole.
    func testAnUnknownNameFallsBackInsteadOfDrawingNothing() {
        XCTAssertNil(FlowerPalette.symbol(forSemanticIcon: "sparkles.rectangle"))
        XCTAssertNil(FlowerPalette.color(forTint: "chartreuse"))

        let inferred = FlowerPalette.inferredIcon(label: "host", kind: "str")
        let unknown = FlowerPalette.icon(label: "host", kind: "str",
                                         icon: "sparkles.rectangle", tint: "chartreuse")
        XCTAssertEqual(unknown.symbol, inferred.symbol)
        XCTAssertEqual(unknown.color, inferred.color)
    }

    // ── what the value points at ──────────────────────────────────────────────

    /// The bargain every defaulted member strikes: a host that resolves nothing
    /// says nothing and keeps conforming.
    func testAHostThatResolvesNothingHasNoLinks() {
        XCTAssertNil(sample().pages.page.items[0].linkLabel)
    }

    /// A resolved reference is read through the protocol — with the stored
    /// property winning over the default even when reached generically, which is
    /// the dispatch mistake this style of test exists to catch.
    func testAResolvedReferenceCrossesTheSeam() {
        func link(of item: some PageItemDisplaying) -> String? { item.linkLabel }
        let ref = SchemaItem(id: "part_of", label: "part_of",
                             preview: "[2026](id:6tzwsxg)",
                             isReadonly: true, linkLabel: "2026")
        XCTAssertEqual(link(of: ref), "2026")
        XCTAssertNil(link(of: SchemaItem(id: "title", label: "title", preview: "A Note")))
    }

    // ── the fold ──────────────────────────────────────────────────────────────

    /// The Swift half of `Page::partitioned`: stable, and each entry keeps the
    /// index it has in the whole item list — which is what `selected` counts,
    /// so the fold rearranges what is drawn without renumbering what is meant.
    func testTheFoldPartitionsStablyAndKeepsDocumentIndices() {
        let items = [
            MetaItem(id: "title", label: "title"),
            MetaItem(id: "updated", label: "updated", demoted: true),
            MetaItem(id: "audience", label: "audience"),
            MetaItem(id: "content_hash", label: "content_hash", demoted: true),
        ]
        let split = partitionDemoted(items)
        XCTAssertEqual(split.promoted.map(\.item.id), ["title", "audience"])
        XCTAssertEqual(split.demoted.map(\.item.id), ["updated", "content_hash"])
        XCTAssertEqual(split.promoted.map(\.index), [0, 2])
        XCTAssertEqual(split.demoted.map(\.index), [1, 3])
    }

    // ── adding what a schema declares ─────────────────────────────────────────

    /// A host that offers nothing keeps exactly the affordances it had: no page
    /// add row, no declared fields, and a keyed add that forwards to the old
    /// path rather than swallowing the tap.
    func testAHostThatOffersNothingKeepsItsOldAddPath() {
        let model = sample()
        XCTAssertFalse(model.canAddChild(pageId: ""))
        XCTAssertTrue(model.addableChildren(of: "").isEmpty)
        model.pageAddChild(id: "exports", key: "audience", value: "public")
        XCTAssertEqual(model.sent, ["add:exports"])
    }

    /// ...and a host that declares fields is read through the protocol: the
    /// offer carries the key, the name and the terms, and a chosen term arrives
    /// as the keyed intent — the field lands already legal.
    func testADeclaredFieldArrivesWithItsKeyAndTerm() {
        func offers<M: PageDriving>(_ model: M) -> [AddableChild] { model.addableChildren(of: "") }
        func showsAddRow<M: PageDriving>(_ model: M) -> Bool { model.canAddChild(pageId: "") }

        let model = DeclaringModel([])
        XCTAssertTrue(showsAddRow(model))
        let declared = offers(model)
        XCTAssertEqual(declared.map(\.key), ["audience"])
        XCTAssertEqual(declared[0].title, "Audience")
        XCTAssertEqual(declared[0].terms, ["public", "private"])

        model.pageAddChild(id: "", key: "audience", value: "public")
        XCTAssertEqual(model.sent, ["add::audience=public"])
    }

    // ── what a row announces ──────────────────────────────────────────────────

    /// The announced name follows the drawn one, step for step: the schema's
    /// title, then the prettified key, then — for a key that cannot be renamed —
    /// the key exactly as the document spells it.
    func testARowAnnouncesTheNameItDraws() {
        XCTAssertEqual(
            rowAccessibilityLabel(SchemaItem(id: "spec", label: "spec",
                                             displayTitle: "Config format version")),
            "Config format version")
        XCTAssertEqual(
            rowAccessibilityLabel(MetaItem(id: "content_hash", label: "content_hash")),
            "Content Hash")
        XCTAssertEqual(
            rowAccessibilityLabel(MetaItem(id: "content_hash", label: "content_hash",
                                           canRename: false)),
            "content_hash")
        // A titled sequence item announces both halves, index first, as drawn.
        XCTAssertEqual(
            rowAccessibilityLabel(MetaItem(id: "steps.0", label: "[0]",
                                           title: "Warm up", canRename: false)),
            "[0], Warm up")
    }

    /// ...and the announced value follows the trailing edge's precedence: where
    /// it goes, what it counts, then what it holds — with silence never an
    /// answer, because "Not set" is drawn for an empty value too.
    func testARowAnnouncesTheValueItDraws() {
        XCTAssertEqual(
            rowAccessibilityValue(SchemaItem(id: "part_of", label: "part_of",
                                             preview: "[2026](id:6tzwsxg)",
                                             linkLabel: "2026")),
            "2026")
        XCTAssertEqual(
            rowAccessibilityValue(MetaItem(id: "exports", label: "exports",
                                           kind: "map", role: "drill",
                                           summary: "{branches: [master]}")),
            "{branches: [master]}")
        XCTAssertEqual(
            rowAccessibilityValue(MetaItem(id: "tags", label: "tags",
                                           kind: "seq", role: "drill", fieldCount: 2)),
            "2 items")
        XCTAssertEqual(
            rowAccessibilityValue(MetaItem(id: "title", label: "title")),
            "Not set")
        XCTAssertEqual(
            rowAccessibilityValue(MetaItem(id: "title", label: "title",
                                           preview: "A Note")),
            "A Note")
    }
}
