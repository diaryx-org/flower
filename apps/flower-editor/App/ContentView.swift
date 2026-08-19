import SwiftUI
import FlowerUI

/// A minimal cross-platform host for the `FlowerUI` structural editor: a header
/// with the document name, a surface switch and a Save control, the editor
/// surface, and a status footer. Everything — the projections, navigation, and
/// the lossless path-addressed edits — comes from flower-core over the FFI; this
/// file is only chrome.
///
/// Two documents and two surfaces, because both choices are the point of the
/// demo. A flat note is the case the settings list was built for; a CI workflow
/// is the case it isn't — nested six deep with a sequence of steps — and the page
/// view is what stays legible there.
struct ContentView: View {
    @StateObject private var note = makeNoteModel()
    @StateObject private var workflow = makeWorkflowModel()
    @State private var sample: Sample = .workflow
    @State private var surface: Surface = .pages

    private var model: FlowerModel { sample == .note ? note : workflow }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            editor
                .background(editorBackground)
            Divider()
            footer
        }
        .ignoresSafeArea(.keyboard, edges: .bottom)
    }

    /// One surface at a time, and each claims its projection when it appears — the
    /// `id` makes the switch a genuine appearance rather than a redraw of the same
    /// view, so a document swap re-enters the page view at the root.
    @ViewBuilder private var editor: some View {
        switch surface {
        case .pages:
            FlowerPages(model: model, rootLabel: sample.rawValue)
                .id("\(sample.rawValue)-pages")
        case .list:
            FlowerEditor(model: model)
                .id("\(sample.rawValue)-list")
        }
    }

    private var header: some View {
        HStack(spacing: 10) {
            Image(systemName: "leaf.fill").foregroundStyle(.green)
            Text("flower").font(.headline)
            Picker("Document", selection: $sample) {
                ForEach(Sample.allCases) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.menu)
            .fixedSize()
            .labelsHidden()
            if model.isDirty {
                Circle().fill(.secondary).frame(width: 6, height: 6)
            }
            Spacer()
            Picker("Surface", selection: $surface) {
                ForEach(Surface.allCases) { Label($0.name, systemImage: $0.symbol).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            Divider().frame(height: 18)
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

    /// Structural editing controls, acting on whatever the visible surface has
    /// selected: the tree's selected row, or the page's selected item. Same
    /// operations either way — the two surfaces differ in what they can show, not
    /// in what they can do.
    @ViewBuilder private var structureControls: some View {
        switch surface {
        case .list: treeControls
        case .pages: pageControls
        }
    }

    @ViewBuilder private var treeControls: some View {
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

    @ViewBuilder private var pageControls: some View {
        let item = model.selectedItem
        Menu {
            Button("Add to this page") { model.pageAddChild(id: model.page.focus) }
            if let item, model.canAddChild(item) {
                Button("Add to \"\(item.title ?? item.label)\"") { model.pageAddChild(id: item.id) }
            }
        } label: {
            Image(systemName: "plus")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .help("Add a field to this page or to the selected container")

        Button {
            if let item { model.moveItemUp(item) }
        } label: { Image(systemName: "arrow.up") }
            .disabled(item == nil)

        Button {
            if let item { model.moveItemDown(item) }
        } label: { Image(systemName: "arrow.down") }
            .disabled(item == nil)

        Button(role: .destructive) {
            if let item { model.delete(item) }
        } label: { Image(systemName: "trash") }
            .disabled(item == nil)
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
            Text(surface == .pages
                 ? "tap a section to open it · right-click a row to rename · add · reorder · delete"
                 : "right-click a row to rename · add · reorder · delete")
                .font(.footnote)
                .foregroundStyle(.tertiary)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 6)
        .background(.bar)
    }
}

/// Which of the two `FlowerUI` surfaces is on screen.
private enum Surface: String, CaseIterable, Identifiable {
    /// The whole document at once, indented — `FlowerEditor`.
    case list
    /// One container at a time, pushed and popped — `FlowerPages`.
    case pages

    var id: Self { self }
    var name: String { self == .list ? "List" : "Pages" }
    var symbol: String { self == .list ? "list.bullet.indent" : "sidebar.right" }
}

/// The demo documents, named by the file they stand in for.
private enum Sample: String, CaseIterable, Identifiable {
    case note = "note.yaml"
    case workflow = "ci.yaml"

    var id: Self { self }
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

private func makeNoteModel() -> FlowerModel {
    // A Diaryx-style note: user fields mixed with prov-managed keys. Flower shows
    // the user's fields and hides the managed ones — they stay in the file
    // byte-for-byte so prov keeps owning them. In a real app this managed-key set
    // comes from a diaryx binding (RelationSet::diaryx() + identity config), not a
    // hardcoded list; here it's inline to demonstrate the mechanism.
    try! FlowerModel(source: sampleNote, format: "yaml", hiddenKeys: diaryxManagedKeys)
}

private func makeWorkflowModel() -> FlowerModel {
    // The deep case: a CI workflow, where the interesting levels are four and five
    // deep and the tree spends most of its width on ancestors.
    try! FlowerModel(source: sampleWorkflow, format: "yaml")
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

private let sampleWorkflow = """
name: CI
on:
  push:
    branches: [master]
  pull_request:
    branches: [master]
concurrency:
  group: ci-${{ github.ref }}
  cancel_in_progress: true
env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1
jobs:
  fmt:
    runs_on: ubuntu-latest
    timeout_minutes: 10
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust
        with:
          toolchain: stable
          components: rustfmt
      - name: Check formatting
        run: cargo fmt --all --check
  test:
    runs_on: macos-latest
    timeout_minutes: 30
    needs: fmt
    strategy:
      fail_fast: false
      matrix:
        rust: [stable, "1.88"]
    steps:
      - uses: actions/checkout@v4
      - name: Install Zig
        with:
          version: 0.14.0
      - name: Run the suite
        run: cargo test --workspace
        env:
          TWIG_SYS_FORCE_SOURCE: 1
"""
