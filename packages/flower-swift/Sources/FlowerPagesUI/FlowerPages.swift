//  FlowerPageView.swift
//
//  The page face of the editor: one container at a time, pushed and popped —
//  flower-core's `page` projection, rendered the way a settings app renders a
//  preference pane.
//
//  `FlowerEditor` (FlowerSettingsView.swift) shows the whole document at once and
//  indents to say what contains what. That reads well until the document is deep,
//  where the useful levels drift right until the keys no longer fit. A page
//  answers the narrower question — "what is *in* this container?" — so depth costs
//  a navigation step instead of a column, and a document nested twelve deep
//  renders exactly as wide as one nested twice.
//
//  Both surfaces drive one model over one document; every edit is path-addressed
//  and lossless either way. Which one to show is a question about the document
//  (and the room), not about what the user is allowed to do.
//
//  ## Two panes, or one — and two different navigations
//
//  When the document has somewhere to drill *and* there is width for it, the panes
//  are consecutive levels of one lineage: the left is the page the right was
//  opened from, at every depth — a window sliding along the trail rather than a
//  fixed sidebar. At the root, where nothing has been opened yet, the right pane
//  previews the page the cursor would open, so the split never starts half empty.
//  A navigation slides them along the trail in the direction it went, which is the
//  only thing distinguishing "one level deeper" from "one level out" once the
//  contents have changed.
//
//  Narrow — a small window, a phone — is one column, and one column pushed and
//  popped *is* a `NavigationStack`. It gets the stack: the OS's push animation,
//  its back button, and on iOS the swipe-back gesture, none of which a hand-rolled
//  breadcrumb can offer. A stack's destination builder is a pull model, though —
//  it asks for the screen at an arbitrary path element, including levels the model
//  is not standing on — so those come from `pageAt(id:)`, which builds a page
//  without navigating to it.
//
//  The two-pane layout keeps its own panes rather than a `NavigationSplitView`,
//  whose sidebar is *fixed*: it would put the root's list next to a page five
//  levels away, which is the arrangement the sliding window exists to replace.
//
//  ## One navigation state, not two
//
//  The stack's path is a mirror, and the model is the original. Focus moves for
//  reasons no tap caused — deleting the container you are inside pops it, a
//  switch from the tree lands wherever the cursor was, a breadcrumb jumps several
//  levels at once — so the path is re-derived from the trail whenever it changes,
//  and a path the *user* changed (a back swipe) is sent back the other way. Each
//  direction checks it has something to say before saying it, which is what stops
//  the two chasing each other.

//  ## Written against protocols, not records
//
//  Nothing here imports a binding. The views take whatever satisfies
//  `PageDriving` and `PageItemDisplaying` (PageProtocols.swift), which the
//  generated `FlowerDoc` records do as they are and a second host's records do
//  with an empty extension. That is the Swift echo of what flower-core already
//  does in Rust, where the projection is generic over the backend and only the
//  UniFFI handle is not.

import SwiftUI

/// Below this the two panes leave neither one usable, so the page view collapses
/// to a single column — the same interaction with one pane instead of two.
private let twoPaneMinWidth: CGFloat = 620

/// The page editor surface: a breadcrumb, then one or two panes of settings rows.
///
/// ```swift
/// FlowerPages(model: model)          // or: FlowerPages(model: model, rootLabel: "note.yaml")
/// ```
///
/// `rootLabel` names the document root in the breadcrumb. flower-core has no name
/// for it — the document is bytes the host opened — so the host supplies one; the
/// TUI calls it `‹document›`.
public struct FlowerPages<Model: PageDriving>: View {
    /// The panes and rows are written in terms of these rather than of the
    /// deeply-nested associated types they unwrap to.
    public typealias Page = Model.Pages.Page
    public typealias Item = Page.Item

    @ObservedObject private var model: Model
    private let theme: FlowerTheme
    private let rootLabel: String

    /// The narrow layout's stack, mirroring the trail. Ids, not pages: a stack
    /// element must survive the document changing underneath it, and an id is the
    /// one thing about a page that does.
    @State private var path: [String] = []
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init(model: Model, theme: FlowerTheme = .default, rootLabel: String = "Document") {
        self.model = model
        self.theme = theme
        self.rootLabel = rootLabel
    }

    public var body: some View {
        GeometryReader { geo in
            if geo.size.width >= twoPaneMinWidth, model.pages.twoPane {
                panes
            } else {
                stack
            }
        }
        .onAppear { model.showPages() }
    }

    // ── the wide layout: two panes sliding along the trail ────────────────────

    private var panes: some View {
        VStack(spacing: 0) {
            breadcrumb
            Divider()
            HStack(spacing: 0) {
                sliding(left, role: .trail)
                Divider()
                rightPane
            }
        }
        // Scoped to the focus, so a navigation animates and an edit — which
        // replaces the same frame in place — does not.
        .animation(reduceMotion ? nil : .easeOut(duration: 0.22),
                   value: model.pages.page.focus)
    }

    /// One pane of the sliding pair.
    ///
    /// Keyed by the page it is showing, so a navigation is an exit and an entrance
    /// rather than a content swap, and stacked rather than laid out side by side:
    /// mid-transition both pages exist, and in an `HStack` that would briefly make
    /// four columns out of two.
    private func sliding(_ page: Page, role: PaneRole) -> some View {
        ZStack {
            PagePane(page: page, model: model, theme: theme,
                     rootLabel: rootLabel, role: role)
                .id(page.focus)
                .transition(paneTransition)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .clipped()
    }

    /// Which way a pane's contents move. A push sends the outgoing page left and
    /// brings the new one in from the right; a pop is the mirror of that. A jump
    /// has no direction — it crosses the trail rather than stepping along it — so
    /// it fades, which is the honest rendering of "somewhere else entirely".
    private var paneTransition: AnyTransition {
        switch model.lastMove {
        case .push:
            return .asymmetric(insertion: .move(edge: .trailing), removal: .move(edge: .leading))
        case .pop:
            return .asymmetric(insertion: .move(edge: .leading), removal: .move(edge: .trailing))
        case .jump:
            return .opacity
        }
    }

    // ── the narrow layout: the OS's own stack ─────────────────────────────────

    /// The trail as the stack sees it: one element per level below the root.
    private var trail: [String] { model.pages.page.crumbs.map(\.id) }

    private var stack: some View {
        NavigationStack(path: $path) {
            screen(id: "")
                .navigationDestination(for: String.self) { screen(id: $0) }
        }
        .onAppear { path = trail }
        // The model moved: mirror it. Guarded, because this also fires for the
        // move the stack itself just made.
        .onChange(of: trail) { moved in
            if path != moved { path = moved }
        }
        // The stack moved — a back button, a swipe — so tell the model where the
        // user actually is. Same guard, other direction.
        .onChange(of: path) { popped in
            guard popped != trail else { return }
            model.pageOpen(id: popped.last ?? "")
        }
    }

    /// One screen of the stack. The page you are standing on comes from the live
    /// frame, with its cursor; every level behind it is built on demand and holds
    /// no cursor, because you are not standing on it.
    @ViewBuilder private func screen(id: String) -> some View {
        let page = id == model.pages.page.focus ? model.pages.page : model.pageAt(id: id)
        PagePane(page: page, model: model, theme: theme, rootLabel: rootLabel, role: .cursor)
            .navigationTitle(id.isEmpty ? rootLabel : prettify(page.crumbs.last?.label ?? id))
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
    }

    /// The left pane: the page the current one was opened from, or — at the root,
    /// which has no parent — the root page itself.
    private var left: Page {
        model.pages.parent ?? model.pages.page
    }

    /// The right pane: the page you are on, or, at the root, the page the cursor
    /// would open. A root selection that opens nothing leaves it empty, which is
    /// honest: there is nothing to show until you pick a section.
    @ViewBuilder private var rightPane: some View {
        if model.pages.parent != nil {
            sliding(model.pages.page, role: .cursor)
        } else if let peek = model.pages.peek {
            sliding(peek, role: .preview)
        } else {
            VStack {
                Spacer()
                Text("Select a section")
                    .font(.system(size: 14))
                    .foregroundStyle(.tertiary)
                Spacer()
            }
            .frame(maxWidth: .infinity)
        }
    }

    /// The trail from the root to the page you are on, each step openable — the
    /// one piece of chrome that says where you are, and the only way back out on a
    /// single-pane layout.
    private var breadcrumb: some View {
        HStack(spacing: 4) {
            Button { model.pageBack() } label: {
                Image(systemName: "chevron.left")
                    .font(.system(size: 12, weight: .semibold))
            }
            .buttonStyle(.plain)
            .disabled(!model.canPageBack)
            .foregroundStyle(model.canPageBack ? Color.accentColor : Color.secondary.opacity(0.4))
            .padding(.trailing, 4)

            // The root's name comes from the host — a file name, usually — so it is
            // shown as given; every crumb below it is a key, which prettifies.
            crumb(label: rootLabel, id: "", isLast: model.pages.page.crumbs.isEmpty)
            ForEach(Array(model.pages.page.crumbs.enumerated()), id: \.element.id) { i, c in
                Image(systemName: "chevron.right")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(.quaternary)
                crumb(label: prettify(c.label), id: c.id,
                      isLast: i == model.pages.page.crumbs.count - 1)
            }
            Spacer()
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 7)
    }

    private func crumb(label: String, id: String, isLast: Bool) -> some View {
        Button { model.pageOpen(id: id) } label: {
            Text(label)
                .font(.system(size: 13, weight: isLast ? .semibold : .regular))
                .foregroundStyle(isLast ? Color.primary : Color.secondary)
        }
        .buttonStyle(.plain)
        .disabled(isLast)
    }
}

// ── One pane ──────────────────────────────────────────────────────────────────

/// What a pane is for, which is what decides how it draws and what it accepts.
private enum PaneRole {
    /// The page the cursor is on: full strength, every affordance live.
    case cursor
    /// The page this one was opened from. It marks the row you came out of — a
    /// trace, not a second cursor — and stays navigable, since going back to a
    /// sibling is the move it exists to make cheap.
    case trail
    /// A preview of the page the cursor *would* open. Nothing has been opened, so
    /// there is nothing here to act on yet.
    case preview

    var isInteractive: Bool { self != .preview }
}

/// One page: its items as a card of settings rows.
private struct PagePane<Model: PageDriving>: View {
    typealias Page = Model.Pages.Page

    let page: Page
    @ObservedObject var model: Model
    let theme: FlowerTheme
    let rootLabel: String
    let role: PaneRole

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 7) {
                if role != .cursor {
                    Text(paneTitle.uppercased())
                        .font(.system(size: 11, weight: .semibold))
                        .tracking(0.6)
                        .foregroundStyle(.tertiary)
                        .padding(.leading, 14)
                }
                if page.items.isEmpty {
                    Text("Empty")
                        .font(.system(size: 14))
                        .foregroundStyle(.tertiary)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 12)
                } else {
                    card
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 14)
            .frame(maxWidth: 560, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .opacity(role == .preview ? 0.55 : 1)
        .allowsHitTesting(role.isInteractive)
    }

    private var paneTitle: String {
        guard let last = page.crumbs.last else { return rootLabel }
        return prettify(last.label)
    }

    private var card: some View {
        VStack(spacing: 0) {
            ForEach(Array(page.items.enumerated()), id: \.element.id) { i, item in
                if item.role == "group" {
                    GroupHeaderRow(item: item, first: i == 0)
                } else {
                    if i > 0, page.items[i - 1].role != "group" {
                        Divider().padding(.leading, 57 + CGFloat(item.inset) * 16)
                    }
                    PageRow(item: item, model: model, theme: theme,
                            selected: page.selected.map { Int($0) == i } ?? false,
                            role: role)
                }
            }
        }
        .background(cardBackground)
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .strokeBorder(Color.primary.opacity(0.06), lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: 14))
    }

    private var cardBackground: some View {
        #if canImport(UIKit)
        Color(.secondarySystemGroupedBackground)
        #else
        Color(nsColor: .controlBackgroundColor)
        #endif
    }
}

/// The header of a container inlined into this page: a caption over the members
/// listed under it. No chevron, though it names a container — its members are
/// already on screen, which is the whole point of inlining them.
private struct GroupHeaderRow<Item: PageItemDisplaying>: View {
    let item: Item
    let first: Bool

    var body: some View {
        HStack(spacing: 8) {
            Text(name.uppercased())
                .font(.system(size: 11, weight: .semibold))
                .tracking(0.5)
                .foregroundStyle(.secondary)
            Rectangle()
                .fill(Color.primary.opacity(0.07))
                .frame(height: 1)
        }
        .padding(.horizontal, 14)
        .padding(.top, first ? 12 : 16)
        .padding(.bottom, 6)
    }

    private var name: String {
        guard let title = item.title else { return prettify(item.label) }
        return "\(prettify(item.label)) · \(title)"
    }
}

/// One row of a page: a scalar edited in place, or a container that opens a page.
///
/// Laid out the way a settings row reads — the name on the left, its value or
/// affordance flushed right — because that is what makes a page scannable: the
/// names form one column and the values another, instead of a ragged `key = value`
/// edge that moves with every key length.
private struct PageRow<Model: PageDriving>: View {
    typealias Item = Model.Pages.Page.Item

    let item: Item
    @ObservedObject var model: Model
    let theme: FlowerTheme
    let selected: Bool
    let role: PaneRole
    @FocusState private var focused: Bool
    @FocusState private var keyFocused: Bool

    private var isEditing: Bool { model.editingId == item.id }
    private var isRenaming: Bool { model.renamingId == item.id }
    private var isDrill: Bool { item.role == "drill" }

    var body: some View {
        HStack(spacing: 12) {
            IconTile(label: item.label, kind: item.kind)
            name
            Spacer(minLength: 8)
            trailing
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .padding(.leading, CGFloat(item.inset) * 16)
        .frame(minHeight: 44)
        .background(selected ? Color.accentColor.opacity(0.12) : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture { model.pageActivate(item) }
        .contextMenu { PageRowMenu(item: item, model: model) }
    }

    /// What names the row. A titled sequence item keeps its index *and* gains the
    /// title: the index is what the path addresses and what a reorder moves, so
    /// dropping it would leave nothing to reconcile the row with the document —
    /// but it is dimmed, because on a list of twenty steps the title is what you
    /// are reading and the index is what you check afterwards.
    @ViewBuilder private var name: some View {
        if isRenaming {
            TextField("key", text: $model.renameBuffer)
                .textFieldStyle(.plain)
                .font(.system(size: 15, weight: .medium))
                .focused($keyFocused)
                .onSubmit { model.commitRename() }
                #if os(macOS)
                .onExitCommand { model.cancelRename() }
                #endif
                .onAppear { keyFocused = true }
                .frame(maxWidth: 180)
        } else if let title = item.title {
            HStack(spacing: 6) {
                Text(item.label)
                    .font(.system(size: 13, design: .monospaced))
                    .foregroundStyle(.tertiary)
                Text(title)
                    .font(.system(size: 15, weight: .medium))
                    .lineLimit(1)
            }
        } else {
            Text(item.canRename ? prettify(item.label) : item.label)
                .font(.system(size: 15))
                .lineLimit(1)
        }
    }

    @ViewBuilder private var trailing: some View {
        if isDrill {
            HStack(spacing: 6) {
                Text(drillText)
                    .font(.system(size: 13, design: item.summary != nil ? .monospaced : .default))
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Image(systemName: "chevron.right")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.tertiary)
            }
        } else if item.kind == "bool" {
            Toggle("", isOn: Binding(get: { item.preview == "true" },
                                     set: { model.setBool(item, $0) }))
                .labelsHidden()
                .toggleStyle(.switch)
        } else if isEditing {
            HStack(spacing: 6) {
                TextField("value", text: $model.editBuffer)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(size: 14, design: numeric ? .monospaced : .default))
                    .frame(maxWidth: 150)
                    .focused($focused)
                    .onSubmit { model.commitEdit() }
                    #if os(macOS)
                    .onExitCommand { model.cancelEdit() }
                    #endif
                    .onAppear { focused = true }
                if numeric {
                    Stepper("", onIncrement: { step(+1) }, onDecrement: { step(-1) })
                        .labelsHidden()
                }
            }
        } else {
            Text(item.preview.isEmpty ? "Not set" : item.preview)
                .font(.system(size: 15, design: valueDesign))
                .foregroundStyle(item.preview.isEmpty
                                 ? Color.gray.opacity(0.8)
                                 : FlowerPalette.value(forKind: item.kind))
                .lineLimit(1)
        }
    }

    /// What a container's row says about it: its contents when they fit, and a
    /// count when they don't.
    ///
    /// `1 field ›` is strictly less than the document says when the field is right
    /// there — the count is the fallback for a container too big to put on a row,
    /// which is the only case where counting beats showing. Core caps the summary
    /// it offers; the row truncates whatever is left over.
    private var drillText: String {
        if let summary = item.summary { return summary }
        let n = item.count
        if item.kind == "seq" { return n == 1 ? "1 item" : "\(n) items" }
        return n == 1 ? "1 field" : "\(n) fields"
    }

    private var numeric: Bool { item.kind == "int" || item.kind == "float" }
    private var valueDesign: Font.Design {
        (item.kind == "str" || item.kind == "null") ? .default : .monospaced
    }

    private func step(_ delta: Int) {
        let t = model.editBuffer.trimmingCharacters(in: .whitespaces)
        if let i = Int(t) { model.editBuffer = String(i + delta) }
        else if let d = Double(t) { model.editBuffer = String(d + Double(delta)) }
    }
}

/// The per-row menu. Every operation takes a path, and a group header still
/// carries one, so a group inlined into this page is as operable as a row that
/// opens its own — inlining is a presentation default, never a cage.
private struct PageRowMenu<Model: PageDriving>: View {
    typealias Item = Model.Pages.Page.Item

    let item: Item
    @ObservedObject var model: Model

    var body: some View {
        if item.role == "scalar" {
            Button("Edit Value") { model.beginEdit(item) }
        }
        if item.role == "drill" {
            Button("Open") { model.pageOpen(id: item.id) }
        }
        if item.canRename {
            Button("Rename") { model.beginRename(item) }
        }
        if model.canAddChild(item) {
            Button(item.kind == "seq" ? "Add Item" : "Add Field") {
                model.pageAddChild(id: item.id)
            }
        }
        Divider()
        Button("Move Up") { model.moveItemUp(item) }
        Button("Move Down") { model.moveItemDown(item) }
        Divider()
        Button("Delete", role: .destructive) { model.delete(item) }
    }
}
