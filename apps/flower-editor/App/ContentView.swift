//  ContentView.swift
//
//  One document's window: the package's `FlowerPages` over the model its
//  `FlowerDocument` owns, with the structural controls in the toolbar and the
//  model's status line under it while it has something to say. Everything —
//  the projection, navigation, and the lossless path-addressed edits — comes
//  from flower-core over the FFI; this file is only chrome.

import FlowerUI
import SwiftUI

struct ContentView<Format: FlowerFormat>: View {
    let document: FlowerDocument<Format>
    /// Where the file is, or nil until it is first saved. Only its name is
    /// used: it labels the root page, as the title bar does the window.
    let fileURL: URL?
    @ObservedObject private var model: FlowerModel

    /// The scene's undo manager is the document's — see
    /// `FlowerDocument.noteEdits(undoManager:)`.
    @Environment(\.undoManager) private var undoManager

    /// How much of the document inlines onto one page before drilling. Shared
    /// by every window and remembered across launches: it is about the room
    /// the pages are drawn in, not about any one file.
    @AppStorage("pages.inlineBudget") private var budget: Budget = .default

    init(document: FlowerDocument<Format>, fileURL: URL?) {
        self.document = document
        self.fileURL = fileURL
        _model = ObservedObject(wrappedValue: document.model)
    }

    var body: some View {
        VStack(spacing: 0) {
            FlowerPages(model: model, rootLabel: fileURL?.lastPathComponent ?? "Untitled")
                .background(editorBackground)
            if !model.status.isEmpty {
                Divider()
                statusBar
            }
        }
        .ignoresSafeArea(.keyboard, edges: .bottom)
        .toolbar { toolbar }
        // The menu bar's commands act on the document in the key window.
        .focusedSceneObject(model)
        .onAppear { model.setInlineBudget(rows: budget.rows, depth: budget.depth) }
        .onChange(of: budget) { model.setInlineBudget(rows: $0.rows, depth: $0.depth) }
        .onChange(of: model.editSeq) { _ in document.noteEdits(undoManager: undoManager) }
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            Picker("Inline", selection: $budget) {
                ForEach(Budget.allCases) { Text($0.name).tag($0) }
            }
            .pickerStyle(.segmented)
            .fixedSize()
            .help("How much of the document inlines onto one page before drilling")
            structureControls
        }
    }

    /// Structural editing controls, acting on whatever the page has selected.
    /// Reordering is not among them: a row is dragged to where it goes, and
    /// Edit ▸ Move Up / Move Down (⌥⌘↑ / ⌥⌘↓) is the keyboard's way.
    @ViewBuilder private var structureControls: some View {
        let item = model.selectedItem
        Menu {
            Button("Add to This Page") { model.pageAddChild(id: model.page.focus) }
            if let item, model.canAddChild(item) {
                Button("Add to \u{201C}\(item.title ?? item.label)\u{201D}") { model.pageAddChild(id: item.id) }
            }
        } label: {
            Label("Add", systemImage: "plus")
        }
        .help("Add a field to this page or to the selected container")

        Button(role: .destructive) {
            if let item { model.delete(item) }
        } label: { Label("Delete", systemImage: "trash") }
            .disabled(item == nil)
            .help("Delete the selected field")
    }

    /// What the last action had to say — chiefly why it was refused. Only
    /// drawn when there is something to say: the window explains nothing about
    /// how to use it, so a bar with nothing in it would be chrome for its own
    /// sake.
    private var statusBar: some View {
        HStack {
            Text(model.status)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            Spacer()
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 6)
        .background(.bar)
    }
}

/// The inline budgets on offer — the ends of the knob and the middle.
private enum Budget: String, CaseIterable, Identifiable {
    /// The settings-menu rule: small all-scalar groups inline.
    case `default`
    /// A couple of ranks, enough for a medium config on few pages.
    case roomy
    /// Effectively unbounded: the whole document on one page.
    case flat

    var id: Self { self }
    var name: String {
        switch self {
        case .default: return "Pages"
        case .roomy: return "Roomy"
        case .flat: return "Flat"
        }
    }
    var rows: Int {
        switch self {
        case .default: return 6
        case .roomy: return 24
        case .flat: return 9999
        }
    }
    var depth: Int {
        switch self {
        case .default: return 1
        case .roomy: return 2
        case .flat: return 99
        }
    }
}

/// The content background, resolved to each toolkit's dynamic system colour so
/// light/dark just works on both platforms.
private var editorBackground: Color {
    #if canImport(UIKit)
    Color(.systemBackground)
    #else
    Color(nsColor: .textBackgroundColor)
    #endif
}
