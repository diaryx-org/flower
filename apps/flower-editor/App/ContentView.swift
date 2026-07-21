import SwiftUI
import FlowerUI

/// A minimal cross-platform host for the `FlowerUI` structural editor: a header
/// with the document name + a Save control, the tree surface, and a status
/// footer. Everything — the tree, navigation, and the lossless path-addressed
/// edits — comes from flower-core over the FFI; this file is only chrome.
struct ContentView: View {
    @StateObject private var model = makeModel()

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            FlowerEditor(model: model)
                .background(editorBackground)
            Divider()
            footer
        }
        .ignoresSafeArea(.keyboard, edges: .bottom)
    }

    private var header: some View {
        HStack(spacing: 10) {
            Image(systemName: "leaf.fill").foregroundStyle(.green)
            Text("flower").font(.headline)
            Text("sample.toml").font(.subheadline).foregroundStyle(.secondary)
            if model.isDirty {
                Circle().fill(.secondary).frame(width: 6, height: 6)
            }
            Spacer()
            structureControls
            Divider().frame(height: 18)
            Button {
                // A real host writes model.source() to disk here; this demo just
                // clears the dirty flag to exercise the round trip.
                _ = model.source()
                model.markSaved()
            } label: {
                Label("Save", systemImage: "square.and.arrow.down")
            }
            .disabled(!model.isDirty)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .background(.bar)
    }

    /// Structural editing controls, acting on the selected row: add a child to a
    /// container, reorder within the parent, delete.
    @ViewBuilder private var structureControls: some View {
        let row = model.selectedRow
        Button {
            if let row { model.addChild(row) }
        } label: { Image(systemName: "plus") }
            .help("Add a key or item to the selected container")
            .disabled(!(row.map(model.canAddChild) ?? false))

        Button {
            if let row { model.moveRowUp(row) }
        } label: { Image(systemName: "arrow.up") }
            .disabled(!(row.map(model.canReorder) ?? false))

        Button {
            if let row { model.moveRowDown(row) }
        } label: { Image(systemName: "arrow.down") }
            .disabled(!(row.map(model.canReorder) ?? false))

        Button(role: .destructive) {
            if let row { model.delete(row) }
        } label: { Image(systemName: "trash") }
            .disabled(row == nil)
    }

    private var footer: some View {
        HStack {
            Text(model.status)
                .font(.footnote)
                .foregroundStyle(.secondary)
            Spacer()
            Text("tap a value to edit · tap a ▸ to fold · right-click to delete")
                .font(.footnote)
                .foregroundStyle(.tertiary)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 6)
        .background(.bar)
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

private func makeModel() -> FlowerModel {
    // The sample is valid TOML, so parsing cannot fail here.
    try! FlowerModel(source: sampleToml, format: "toml")
}

private let sampleToml = """
# flower sample config — comments and formatting survive edits
title = "flower"
version = 1
enabled = true

# the server block
[server]
host = "localhost"
port = 8080
tags = ["alpha", "beta"]

[server.limits]
max_connections = 100
timeout = 30.5
"""
