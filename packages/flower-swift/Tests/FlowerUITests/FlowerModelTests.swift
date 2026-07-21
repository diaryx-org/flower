import XCTest
@testable import FlowerUI
import FlowerFFI

/// Exercises the FlowerUI model against the real flower-core over the FFI (the
/// staticlib is force-loaded by scripts/test-swift.sh) plus the pure-Swift theme.
final class FlowerModelTests: XCTestCase {
    let sample = """
    title = "flower"
    version = 1
    enabled = true

    [server]
    host = "localhost"
    port = 8080
    tags = ["alpha", "beta"]
    """

    func makeModel() throws -> FlowerModel {
        try FlowerModel(source: sample, format: "toml")
    }

    func testViewListsTopLevelRows() throws {
        let model = try makeModel()
        let labels = model.state.rows.map { $0.label }
        XCTAssertTrue(labels.contains("title"))
        XCTAssertTrue(labels.contains("server"))
        XCTAssertFalse(model.isDirty)
    }

    func testEditingAScalarIsLosslessAndDirties() throws {
        let model = try makeModel()
        guard let row = model.state.rows.first(where: { $0.id == "version" }) else {
            return XCTFail("no version row")
        }
        model.beginEdit(row)
        XCTAssertEqual(model.editBuffer, "1")
        model.editBuffer = "2"
        model.commitEdit()

        XCTAssertTrue(model.isDirty)
        XCTAssertTrue(model.source().contains("version = 2"))
        XCTAssertTrue(model.source().contains("title = \"flower\""), "sibling preserved")
    }

    func testTogglingAContainerCollapsesItsChildren() throws {
        let model = try makeModel()
        guard let server = model.state.rows.first(where: { $0.id == "server" }) else {
            return XCTFail("no server row")
        }
        model.toggle(server)
        XCTAssertFalse(model.state.rows.contains { $0.id == "server.host" })
        model.toggle(server)
        XCTAssertTrue(model.state.rows.contains { $0.id == "server.host" })
    }

    func testDeleteRemovesAKey() throws {
        let model = try makeModel()
        guard let row = model.state.rows.first(where: { $0.id == "enabled" }) else {
            return XCTFail("no enabled row")
        }
        model.delete(row)
        XCTAssertFalse(model.source().contains("enabled = true"))
        XCTAssertTrue(model.source().contains("title = \"flower\""))
    }

    func testAddChildAppendsToASequenceAndOpensItForEditing() throws {
        let model = try makeModel()
        guard let tags = model.state.rows.first(where: { $0.id == "server.tags" }) else {
            return XCTFail("no tags row")
        }
        model.addChild(tags)
        XCTAssertTrue(model.state.rows.contains { $0.id == "server.tags.2" })
        XCTAssertEqual(model.editingId, "server.tags.2", "new item opens for editing")
        model.editBuffer = "gamma"
        model.commitEdit()
        XCTAssertTrue(model.source().contains("gamma"))
    }

    func testAddChildInsertsAKeyIntoAMapping() throws {
        let model = try makeModel()
        guard let server = model.state.rows.first(where: { $0.id == "server" }) else {
            return XCTFail("no server row")
        }
        model.addChild(server)
        XCTAssertTrue(model.state.rows.contains { $0.id == "server.new_key" })
        XCTAssertEqual(model.editingId, "server.new_key")
    }

    func testMoveRowReordersWithinParent() throws {
        let model = try makeModel()
        guard let beta = model.state.rows.first(where: { $0.id == "server.tags.1" }) else {
            return XCTFail("no tags[1] row")
        }
        model.moveRowUp(beta)
        let src = model.source()
        XCTAssertLessThan(src.range(of: "beta")!.lowerBound, src.range(of: "alpha")!.lowerBound)
    }

    func testSetBoolCommitsImmediately() throws {
        let model = try makeModel()
        guard let enabled = model.state.rows.first(where: { $0.id == "enabled" }) else {
            return XCTFail("no enabled row")
        }
        model.setBool(enabled, false)
        XCTAssertTrue(model.source().contains("enabled = false"))
    }

    func testThemeColoursValuesByKind() {
        let theme = FlowerTheme.default
        // Distinct kinds map to distinct colours; containers use chrome (secondary).
        XCTAssertEqual(theme.symbol(isContainer: true, expanded: true), "chevron.down")
        XCTAssertEqual(theme.symbol(isContainer: true, expanded: false), "chevron.right")
        XCTAssertNotEqual(theme.color(forKind: "str"), theme.color(forKind: "int"))
    }
}
