//  FlowerDocument.swift
//
//  A file on disk, as the document system sees it. `DocumentGroup` owns the
//  rest — Open, Save, Save As, Duplicate, Rename, Revert, autosave, Versions,
//  the recents list and the title bar's proxy icon on the Mac; the document
//  browser and the Files app on iOS — and asks this class only three things:
//  read these bytes, write these bytes, and tell me when something changed.
//
//  One class per format rather than one class for all of them, because a
//  document's format is a fact about its bytes that never changes: flower
//  parses TOML into TOML and writes TOML back, and core offers no conversion.
//  A single class listing every type it can read would make Save As offer a
//  Format pop-up whose other choices would write TOML under a `.json`
//  extension. With one type per class the pop-up never appears, and File ▸ New
//  becomes a submenu naming each. Leaf's documents are split the same way.

import FlowerUI
import SwiftUI
import UniformTypeIdentifiers

extension UTType {
    /// `public.toml` is declared by the system on both platforms, but has no
    /// Swift name of its own; `.toml` already resolves to it.
    static let tomlDocument = UTType("public.toml")!
    /// fig has no system type, so `project.yml` imports one under this
    /// identifier — `.fig` and `.figl` — and this is its Swift name.
    static let figDocument = UTType(importedAs: "org.diaryx.fig")
}

/// What one format's document class says about itself.
protocol FlowerFormat {
    /// The name core parses by — see `FlowerModel(source:format:)`.
    static var name: String { get }
    static var contentType: UTType { get }
    /// What File ▸ New starts from: the smallest source that parses to an
    /// empty mapping, so the first Add has somewhere to go. An empty file is
    /// that for TOML and fig, and is no document at all — or a scalar — for
    /// the rest.
    static var empty: String { get }
}

enum JSONFormat: FlowerFormat {
    static let name = "json"
    static let contentType = UTType.json
    static let empty = "{}\n"
}

enum YAMLFormat: FlowerFormat {
    static let name = "yaml"
    static let contentType = UTType.yaml
    static let empty = "{}\n"
}

enum TOMLFormat: FlowerFormat {
    static let name = "toml"
    static let contentType = UTType.tomlDocument
    static let empty = ""
}

enum FigFormat: FlowerFormat {
    static let name = "fig"
    static let contentType = UTType.figDocument
    static let empty = ""
}

// Concrete classes rather than type aliases: `DocumentGroup` tells its groups
// apart by class, and specialisations of one generic read as one.
final class JSONDocument: FlowerDocument<JSONFormat> {}
final class YAMLDocument: FlowerDocument<YAMLFormat> {}
final class TOMLDocument: FlowerDocument<TOMLFormat> {}
final class FigDocument: FlowerDocument<FigFormat> {}

/// A config file in one format, holding the model the window edits.
///
/// A reference document, not a value one: the model is a class that owns a
/// live FFI handle, and the page editor edits it in place. Its history is the
/// model's journal, not a second one kept here — `noteEdits(undoManager:)`
/// only tells the window's undo manager that the journal moved, with actions
/// that walk the journal back and forward.
class FlowerDocument<Format: FlowerFormat>: ReferenceFileDocument {
    typealias Snapshot = String

    static var readableContentTypes: [UTType] { [Format.contentType] }
    static var writableContentTypes: [UTType] { [Format.contentType] }

    let model: FlowerModel

    /// The model's `editSeq` the undo manager has already been told about.
    private var seenSeq: UInt64

    /// A new, empty document.
    required init() {
        // Every format's `empty` parses to an empty mapping.
        model = try! FlowerModel(source: Format.empty, format: Format.name)
        seenSeq = model.editSeq
    }

    required init(configuration: ReadConfiguration) throws {
        guard let data = configuration.file.regularFileContents,
              let source = String(data: data, encoding: .utf8)
        else { throw CocoaError(.fileReadCorruptFile) }
        // A file that does not parse is refused at the door, with fig's
        // reason: flower edits a tree, and there is none to edit.
        model = try FlowerModel(source: source, format: Format.name)
        seenSeq = model.editSeq
    }

    /// Serialised on the main thread while the model is quiet; the document
    /// system then writes it from wherever it likes.
    func snapshot(contentType: UTType) throws -> String {
        model.source()
    }

    func fileWrapper(snapshot: String, configuration: WriteConfiguration) throws -> FileWrapper {
        // The snapshot is what is going to disk, and an edit made since it was
        // taken is not — which is exactly what `markSaved(as:)` records.
        DispatchQueue.main.async { [model] in model.markSaved(as: snapshot) }
        return FileWrapper(regularFileWithContents: Data(snapshot.utf8))
    }

    // ── the window's undo manager ─────────────────────────────────────────────

    /// Tell `undoManager` of every edit the model has committed since it was
    /// last told. Called whenever `editSeq` moves.
    ///
    /// The scene's undo manager is the document's: an action registered on it
    /// is how a `ReferenceFileDocument` says it is edited — what enables Save,
    /// shows the dot in the close button and starts the autosave clock — and
    /// an action undone is how it says the edit went away, so undoing back to
    /// the saved text reads as clean again. Each action replays the model's
    /// own journal, so Edit ▸ Undo and ⌘Z walk the history flower already
    /// keeps.
    func noteEdits(undoManager: UndoManager?) {
        let seq = model.editSeq
        defer { seenSeq = seq }
        guard let undoManager, seq > seenSeq else { return }
        // Edits coalesced into one view update still count one by one, so the
        // two journals stay the same length.
        for _ in seenSeq..<seq { registerUndo(on: undoManager) }
    }

    private func registerUndo(on undoManager: UndoManager) {
        undoManager.registerUndo(withTarget: self) { document in
            document.replay(document.model.undo)
            // Registered while undoing, so it lands on the redo stack.
            document.registerRedo(on: undoManager)
        }
    }

    private func registerRedo(on undoManager: UndoManager) {
        undoManager.registerUndo(withTarget: self) { document in
            document.replay(document.model.redo)
            document.registerUndo(on: undoManager)
        }
    }

    /// Run an undo or redo, and count the `editSeq` it moved as already told:
    /// it is the undo manager's own action, not a fresh edit to register.
    private func replay(_ step: () -> Void) {
        step()
        seenSeq = model.editSeq
    }
}
