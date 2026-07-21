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
            Text("note.yaml").font(.subheadline).foregroundStyle(.secondary)
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

    /// Structural editing controls, acting on the selected row: add (to root or a
    /// selected container), reorder within the parent, and delete.
    @ViewBuilder private var structureControls: some View {
        let row = model.selectedRow
        Menu {
            Button("Add top-level field") { model.addRootChild() }
            if let row, model.canAddChild(row) {
                Button("Add to \"\(row.label)\"") { model.addChild(row) }
            }
        } label: {
            Image(systemName: "plus")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .help("Add a field to the document or the selected container")

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
        HStack(spacing: 10) {
            Text(model.status)
                .font(.footnote)
                .foregroundStyle(.secondary)
            if model.hiddenCount > 0 {
                Label("\(model.hiddenCount) managed fields hidden", systemImage: "lock.fill")
                    .font(.footnote)
                    .foregroundStyle(.tertiary)
                    .help("Prov-managed keys (id, prov, contents, …) stay in the file but aren't shown or editable here.")
            }
            Spacer()
            Text("right-click a row to rename · add · reorder · delete")
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
    // A Diaryx-style note: user fields mixed with prov-managed keys. Flower shows
    // the user's fields and hides the managed ones — they stay in the file
    // byte-for-byte so prov keeps owning them. In a real app this managed-key set
    // comes from a diaryx binding (RelationSet::diaryx() + identity config), not a
    // hardcoded list; here it's inline to demonstrate the mechanism.
    try! FlowerModel(source: sampleNote, format: "yaml", hiddenKeys: diaryxManagedKeys)
}

private let diaryxManagedKeys = [
    "contents", "part_of", "links", "link_of", "registry",
    "config", "recycle_bin", "id", "title", "prov",
]

private let sampleNote = """
# Diaryx note — the managed keys below (id, title, prov, contents, …) are hidden
# by flower; they remain in the file and prov keeps owning them.
id: 01JQ8Z9K7M4RN0P2
title: My First Note
author: alice
tags:
  - journal
  - draft
visibility: private
pinned: false
priority: 3
contents: []
part_of: []
prov:
  version: 3
  fixity: sha256-abc123
"""
