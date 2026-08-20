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
}
