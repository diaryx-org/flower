//  FlowerApp.swift
//
//  Flower, the config editor: a window per file, one `FlowerDocument` behind
//  each. `DocumentGroup` is the whole of the file story — the Open panel at
//  launch, File ▸ New / Open / Save / Save As / Duplicate / Rename / Revert,
//  autosave and Versions, the recents list, the proxy icon in the title bar,
//  Finder's "Open With" once `project.yml` has declared the types; on iOS the
//  document browser and the Files app. Everything this file adds is the chrome
//  around that.

import FlowerUI
import SwiftUI

@main
struct FlowerApp: App {
    var body: some Scene {
        // One group per format, so a document's type is fixed at birth (see
        // `FlowerDocument`). The first is what File ▸ New and the iOS launch
        // screen's Create make; the others hang under New as a submenu.
        DocumentGroup(newDocument: { TOMLDocument() }) { file in
            ContentView(document: file.document, fileURL: file.fileURL).sized()
        }
        .documentWindowSize()
        DocumentGroup(newDocument: { JSONDocument() }) { file in
            ContentView(document: file.document, fileURL: file.fileURL).sized()
        }
        .documentWindowSize()
        DocumentGroup(newDocument: { YAMLDocument() }) { file in
            ContentView(document: file.document, fileURL: file.fileURL).sized()
        }
        .documentWindowSize()
        DocumentGroup(newDocument: { FigDocument() }) { file in
            ContentView(document: file.document, fileURL: file.fileURL).sized()
        }
        .documentWindowSize()
        .commands {
            FlowerAppCommands()
        }
    }
}

private extension Scene {
    /// A settings list is tall and narrow: two panes of keys and values side
    /// by side, and as many rows as fit.
    func documentWindowSize() -> some Scene {
        #if os(macOS)
        defaultSize(width: 760, height: 640)
        #else
        self
        #endif
    }
}

private extension View {
    func sized() -> some View {
        #if os(macOS)
        frame(minWidth: 420, minHeight: 320)
        #else
        self
        #endif
    }
}

/// The app's own menu items, beside what `DocumentGroup` provides.
struct FlowerAppCommands: Commands {
    /// The model of the document in the key window, if there is one.
    @FocusedObject private var model: FlowerModel?

    var body: some Commands {
        // In Edit, beside the other things done to a selection. Also how an
        // iPad with a keyboard reorders, and the way to do it without a drag.
        CommandGroup(after: .pasteboard) {
            Divider()
            Button("Move Up") { if canMove, let model, let item = model.selectedItem { model.moveItemUp(item) } }
                .keyboardShortcut(.upArrow, modifiers: [.option, .command])
                .disabled(!canMove)
            Button("Move Down") { if canMove, let model, let item = model.selectedItem { model.moveItemDown(item) } }
                .keyboardShortcut(.downArrow, modifiers: [.option, .command])
                .disabled(!canMove)
        }
        #if os(macOS)
        CommandGroup(after: .help) {
            Divider()
            Link("Flower on GitHub",
                 destination: URL(string: "https://github.com/diaryx-org/flower")!)
        }
        #endif
    }

    /// Whether the selected row can be moved now: not while a value or key is
    /// being typed — the arrows belong to the text field then — and not a
    /// row something else maintains.
    private var canMove: Bool {
        guard let model, model.editingId == nil, model.renamingId == nil,
              let item = model.selectedItem else { return false }
        return !item.isReadonly
    }
}
