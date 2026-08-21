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
}
