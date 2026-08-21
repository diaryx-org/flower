import XCTest
@testable import FlowerUI
import FlowerFFI

/// Exercises the page projection through `FlowerModel` — the surface `FlowerPages`
/// renders — against the real flower-core over the FFI.
final class FlowerPageTests: XCTestCase {
    /// Deep enough to have somewhere to drill: a nested mapping, a small group
    /// that inlines, and a sequence of mappings the titles have to name.
    let sample = """
    name: flower
    server:
      host: localhost
      port: 8080
      limits:
        max_connections: 100
        timeout: 30
    jobs:
      - name: build
        runs_on: macos
      - name: test
        runs_on: linux
    """

    func makeModel() throws -> FlowerModel {
        let model = try FlowerModel(source: sample, format: "yaml")
        model.showPages()
        return model
    }

    private func ids(_ page: PageView) -> [String] { page.items.map(\.id) }

    func testTheRootPageListsOneLevel() throws {
        let model = try makeModel()
        XCTAssertEqual(ids(model.page), ["name", "server", "jobs"])
        XCTAssertTrue(model.pages.twoPane, "there is something to drill into")
        XCTAssertNil(model.pages.parent, "the root came out of nothing")
        XCTAssertTrue(model.page.crumbs.isEmpty)
        XCTAssertFalse(model.canPageBack)
    }

    func testAFlatDocumentAsksForOnePane() throws {
        let model = try FlowerModel(source: "a = 1\nb = 2\n", format: "toml")
        model.showPages()
        XCTAssertFalse(model.pages.twoPane)
    }

    func testOpeningADrillPushesAPageAndBackPopsIt() throws {
        let model = try makeModel()
        model.pageOpen(id: "server")
        XCTAssertEqual(model.page.focus, "server")
        XCTAssertEqual(model.page.crumbs.map(\.label), ["server"])
        XCTAssertEqual(model.pages.parent?.focus, "", "the left pane is the root")
        XCTAssertTrue(model.canPageBack)

        model.pageBack()
        XCTAssertEqual(model.page.focus, "")
        XCTAssertEqual(model.selectedItem?.id, "server", "the cursor returns to it")
    }

    func testASmallGroupIsInlinedWithItsMembersUnderIt() throws {
        let model = try makeModel()
        model.pageOpen(id: "server")
        XCTAssertEqual(ids(model.page), [
            "server.host", "server.port", "server.limits",
            "server.limits.max_connections", "server.limits.timeout",
        ])
        let group = model.page.items.first { $0.id == "server.limits" }
        XCTAssertEqual(group?.role, "group")
        XCTAssertEqual(model.page.items.last?.inset, 1)

        // A group header opens no page of its own — its members are already here.
        model.pageOpen(id: "server.limits")
        XCTAssertEqual(model.page.focus, "server")
    }

    func testSequenceItemsAreNamedByWhatIsInThem() throws {
        let model = try makeModel()
        model.pageOpen(id: "jobs")
        let first = model.page.items.first { $0.id == "jobs.0" }
        XCTAssertEqual(first?.label, "[0]", "the index is what the path addresses")
        XCTAssertEqual(first?.title, "build")
    }

    func testEditingFromAPageIsLosslessAndRoutesByProjection() throws {
        let model = try makeModel()
        model.pageOpen(id: "server")
        guard let port = model.page.items.first(where: { $0.id == "server.port" }) else {
            return XCTFail("no port row")
        }
        model.beginEdit(port)
        XCTAssertEqual(model.editBuffer, "8080")
        XCTAssertEqual(model.editingId, "server.port")
        model.editBuffer = "9090"
        model.commitEdit()

        XCTAssertTrue(model.isDirty)
        XCTAssertTrue(model.source().contains("port: 9090"))
        XCTAssertTrue(model.source().contains("host: localhost"), "sibling untouched")
    }

    func testTheBoolRowCommitsImmediately() throws {
        let model = try FlowerModel(source: "server:\n  tls: false\n  port: 8080\n", format: "yaml")
        model.showPages()
        model.pageOpen(id: "server")
        guard let tls = model.page.items.first(where: { $0.id == "server.tls" }) else {
            return XCTFail("no tls row")
        }
        model.setBool(tls, true)
        XCTAssertTrue(model.source().contains("tls: true"))
    }

    func testAddingToThePageOpensTheNewFieldForEditing() throws {
        let model = try makeModel()
        model.pageOpen(id: "server")
        model.pageAddChild(id: model.page.focus)
        XCTAssertTrue(model.page.items.contains { $0.id == "server.new_key" })
        XCTAssertEqual(model.editingId, "server.new_key", "the new field opens for editing")
    }

    func testDeletingAndReorderingAddressTheRowTapped() throws {
        let model = try makeModel()
        model.pageOpen(id: "jobs")
        guard let second = model.page.items.first(where: { $0.id == "jobs.1" }) else {
            return XCTFail("no second job")
        }
        model.moveItemUp(second)
        let src = model.source()
        XCTAssertLessThan(src.range(of: "test")!.lowerBound, src.range(of: "build")!.lowerBound)

        guard let first = model.page.items.first(where: { $0.id == "jobs.0" }) else {
            return XCTFail("no first job")
        }
        model.delete(first)
        XCTAssertFalse(model.source().contains("test"))
    }

    func testRaisingTheInlineBudgetInlinesTheDocumentOntoTheRootPage() throws {
        let model = try makeModel()
        model.setInlineBudget(rows: 99, depth: 8)

        // Everything inlines: `server` is a group on the root page, `limits` a
        // group one rank in, its members two ranks in — the settings-list
        // rendering, from the same projection.
        XCTAssertEqual(model.page.focus, "")
        XCTAssertEqual(item(model, "server")?.role, "group")
        XCTAssertEqual(item(model, "server.limits")?.role, "group")
        XCTAssertEqual(item(model, "server.limits.timeout")?.inset, 2)
        XCTAssertFalse(model.pages.twoPane, "nothing left to drill into")

        // Edits keep addressing the same paths at any budget.
        guard let timeout = item(model, "server.limits.timeout") else {
            return XCTFail("no timeout row")
        }
        model.beginEdit(timeout)
        model.editBuffer = "45"
        model.commitEdit()
        XCTAssertTrue(model.source().contains("timeout: 45"))

        // Back at the default, depth costs a page again.
        model.setInlineBudget(rows: 6, depth: 1)
        XCTAssertEqual(item(model, "server")?.role, "drill")
    }

    private func item(_ model: FlowerModel, _ id: String) -> PageItemView? {
        model.page.items.first { $0.id == id }
    }

    // ── navigation state the two layouts read ─────────────────────────────────

    /// Three drillable levels, so the trail has a step no live pane lists.
    let nested = """
    a:
      b:
        c:
          one: 1
          two: 2
          more:
            x: 1
        other:
          p: 1
          q: 2
      b2:
        p: 1
    """

    func makeNested() throws -> FlowerModel {
        let model = try FlowerModel(source: nested, format: "yaml")
        model.showPages()
        return model
    }

    func testTheTrailIsTheStacksPath() throws {
        let model = try makeNested()
        XCTAssertEqual(model.page.crumbs.map(\.id), [])
        model.pageOpen(id: "a")
        model.pageOpen(id: "a.b")
        XCTAssertEqual(model.page.crumbs.map(\.id), ["a", "a.b"])
    }

    func testAMiddleBreadcrumbOpensThePageItNames() throws {
        let model = try makeNested()
        model.pageOpen(id: "a")
        model.pageOpen(id: "a.b")
        model.pageOpen(id: "a.b.c")
        // `a.b` is on no live pane — this page lists c's fields, the pane beside
        // it lists b's, the root lists `a` — so it resolves as a step of the trail.
        model.pageOpen(id: "a.b")
        XCTAssertEqual(model.page.focus, "a.b")
    }

    func testLastMoveNamesTheDirectionOfTravel() throws {
        let model = try makeNested()
        model.pageOpen(id: "a")
        XCTAssertEqual(model.lastMove, .push)
        model.pageOpen(id: "a.b")
        XCTAssertEqual(model.lastMove, .push)
        model.pageBack()
        XCTAssertEqual(model.lastMove, .pop)

        // Several levels at once has no direction to animate along.
        model.pageOpen(id: "a.b")
        model.pageOpen(id: "a.b.c")
        model.pageOpen(id: "")
        XCTAssertEqual(model.lastMove, .jump)
    }

    func testAnEditDoesNotCountAsAMove() throws {
        let model = try makeNested()
        model.pageOpen(id: "a")
        model.pageOpen(id: "a.b")
        model.pageOpen(id: "a.b.c")
        XCTAssertEqual(model.lastMove, .push)
        guard let one = model.page.items.first(where: { $0.id == "a.b.c.one" }) else {
            return XCTFail("no `one` row")
        }
        model.beginEdit(one)
        model.editBuffer = "42"
        model.commitEdit()
        XCTAssertEqual(model.lastMove, .push, "the frame changed, the level did not")
    }

    func testPageAtRendersALevelWithoutNavigatingToIt() throws {
        let model = try makeNested()
        model.pageOpen(id: "a")
        model.pageOpen(id: "a.b")
        model.pageOpen(id: "a.b.c")

        // Every screen a stack could ask for, including the levels behind us.
        XCTAssertEqual(model.pageAt(id: "").items.first?.id, "a")
        XCTAssertEqual(model.pageAt(id: "a").items.first?.id, "a.b")
        XCTAssertEqual(model.pageAt(id: "a.b").items.first?.id, "a.b.c")
        XCTAssertNil(model.pageAt(id: "a.b").selected, "an ancestor holds no cursor")
        XCTAssertEqual(model.pageAt(id: "a.b.c").selected, 0, "the live page keeps its cursor")

        XCTAssertEqual(model.page.focus, "a.b.c", "and rendering moved nothing")
    }
}
