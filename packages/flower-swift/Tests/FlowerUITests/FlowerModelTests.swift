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
        let model = try FlowerModel(source: sample, format: "toml")
        model.showPages()
        return model
    }

    private func item(_ model: FlowerModel, _ id: String) -> PageItemView? {
        model.page.items.first { $0.id == id }
    }

    func testTheRootPageListsTopLevelEntries() throws {
        let model = try makeModel()
        let labels = model.page.items.filter { $0.inset == 0 }.map(\.label)
        XCTAssertTrue(labels.contains("title"))
        XCTAssertTrue(labels.contains("server"))
        XCTAssertFalse(model.isDirty)
        XCTAssertEqual(model.rootKind, "map")
    }

    func testEditingAScalarIsLosslessAndDirties() throws {
        let model = try makeModel()
        guard let version = item(model, "version") else {
            return XCTFail("no version row")
        }
        model.beginEdit(version)
        XCTAssertEqual(model.editBuffer, "1")
        model.editBuffer = "2"
        model.commitEdit()

        XCTAssertTrue(model.isDirty)
        XCTAssertTrue(model.source().contains("version = 2"))
        XCTAssertTrue(model.source().contains("title = \"flower\""), "sibling preserved")
    }

    func testMarkSavedClearsTheDirtyFlag() throws {
        let model = try makeModel()
        guard let version = item(model, "version") else {
            return XCTFail("no version row")
        }
        model.beginEdit(version)
        model.editBuffer = "3"
        model.commitEdit()
        XCTAssertTrue(model.isDirty)

        model.markSaved()
        XCTAssertFalse(model.isDirty)
    }

    func testDeleteRemovesAKey() throws {
        let model = try makeModel()
        guard let enabled = item(model, "enabled") else {
            return XCTFail("no enabled row")
        }
        model.delete(enabled)
        XCTAssertFalse(model.source().contains("enabled = true"))
        XCTAssertTrue(model.source().contains("title = \"flower\""))
    }

    func testAddingToTheRootPageOpensTheNewFieldForEditing() throws {
        let model = try makeModel()
        model.pageAddChild(id: "")
        XCTAssertTrue(model.page.items.contains { $0.id == "new_key" })
        XCTAssertEqual(model.editingId, "new_key", "the new field opens for editing")
        // A second add picks a key that does not collide with the first.
        model.commitEdit()
        model.pageAddChild(id: "")
        XCTAssertTrue(model.page.items.contains { $0.id == "new_key2" })
    }

    func testAddingToAnInlinedSequenceAppendsAndOpensTheNewItem() throws {
        let model = try makeModel()
        model.pageOpen(id: "server")
        model.pageAddChild(id: "server.tags")
        XCTAssertTrue(model.page.items.contains { $0.id == "server.tags.2" })
        XCTAssertEqual(model.editingId, "server.tags.2", "new item opens for editing")
        model.editBuffer = "gamma"
        model.commitEdit()
        XCTAssertTrue(model.source().contains("gamma"))
    }

    func testSetBoolCommitsImmediately() throws {
        let model = try makeModel()
        guard let enabled = item(model, "enabled") else {
            return XCTFail("no enabled row")
        }
        model.setBool(enabled, false)
        XCTAssertTrue(model.source().contains("enabled = false"))
    }

    func testHiddenKeysAreProjectedOutButKept() throws {
        let model = try FlowerModel(source: sample, format: "toml", hiddenKeys: ["title", "enabled"])
        model.showPages()
        XCTAssertFalse(model.page.items.contains { $0.id == "title" })
        XCTAssertFalse(model.page.items.contains { $0.id == "enabled" })
        XCTAssertTrue(model.page.items.contains { $0.id == "version" })
        XCTAssertEqual(model.hiddenCount, 2)
        XCTAssertTrue(model.source().contains("title = \"flower\""))
    }

    func testRenameKeyKeepsValue() throws {
        let model = try makeModel()
        guard let version = item(model, "version") else {
            return XCTFail("no version row")
        }
        model.beginRename(version)
        XCTAssertEqual(model.renameBuffer, "version")
        model.renameBuffer = "revision"
        model.commitRename()
        XCTAssertTrue(model.page.items.contains { $0.id == "revision" })
        XCTAssertTrue(model.source().contains("= 1"))
    }

    func testThemeColoursValuesByKind() {
        let theme = FlowerTheme.default
        // Distinct kinds map to distinct colours; containers use chrome (secondary).
        XCTAssertEqual(theme.symbol(isContainer: true, expanded: true), "chevron.down")
        XCTAssertEqual(theme.symbol(isContainer: true, expanded: false), "chevron.right")
        XCTAssertNotEqual(theme.color(forKind: "str"), theme.color(forKind: "int"))
    }
}
