import SwiftUI
import FlowerUI

/// A minimal cross-platform host for the `FlowerUI` page editor: a header with
/// the document name, an inline-budget control and a Save control, the editor
/// surface, and a status footer. Everything — the projection, navigation, and
/// the lossless path-addressed edits — comes from flower-core over the FFI; this
/// file is only chrome.
///
/// Two documents, because both ends of the inline budget are the point of the
/// demo. A flat note inlines onto one page at any budget; a CI workflow —
/// nested six deep with a sequence of steps — drills at the default and flattens
/// as the budget grows, which is the one knob where a second surface used to be.
struct ContentView: View {
    @StateObject private var note = makeNoteModel()
    @StateObject private var workflow = makeWorkflowModel()
    @State private var sample: Sample = .workflow
    @State private var budget: Budget = .default

    private var model: FlowerModel { sample == .note ? note : workflow }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            // The `id` makes a document swap a genuine appearance rather than a
            // redraw of the same view, so it re-enters the page view at the root.
            FlowerPages(model: model, rootLabel: sample.rawValue)
                .id(sample.rawValue)
                .background(editorBackground)
            Divider()
            footer
        }
        .ignoresSafeArea(.keyboard, edges: .bottom)
        .onChange(of: budget) { model.setInlineBudget(rows: $0.rows, depth: $0.depth) }
        .onChange(of: sample) { _ in model.setInlineBudget(rows: budget.rows, depth: budget.depth) }
    }

    private var header: some View {
        HStack(spacing: 10) {
            Image(systemName: "leaf.fill").foregroundStyle(.green)
            // Fixed, so a narrow window compresses the controls to its right
            // rather than setting the wordmark one letter per line.
            Text("flower").font(.headline).fixedSize()
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
            Picker("Inline", selection: $budget) {
                ForEach(Budget.allCases) { Text($0.name).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            .help("How much of the document inlines onto one page before drilling")
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

    /// Structural editing controls, acting on whatever the page has selected.
    @ViewBuilder private var structureControls: some View {
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
            Text("tap a section to open it · right-click a row to rename · add · reorder · delete")
                .font(.footnote)
                .foregroundStyle(.tertiary)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 6)
        .background(.bar)
    }
}

/// The inline budgets the demo offers — the ends of the knob and the middle.
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
    // deep and the page view is what stays legible.
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
