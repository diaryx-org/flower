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

/// Who owns the back gesture when the page view is one column wide.
///
/// One column pushed and popped *is* a `NavigationStack`, so by default it gets
/// one and inherits the OS's push animation, its back button, and on iOS the
/// swipe-back gesture — none of which a hand-rolled breadcrumb can offer.
///
/// That reasoning assumes the page view brought its own navigation context. A
/// host that already has one — a macOS Settings scene, most sharply, where the
/// window itself pushes and pops and its back button returns to the settings
/// root — ends up with two, and the one the user reaches is the outer one: the
/// back button leaves the whole pane instead of stepping out of the container
/// they opened, and the page they were standing on is not on the way back.
///
/// The distinction is the host's to make because it is a fact about the *scene*,
/// not about the document or the width. Nothing else changes: the same pages,
/// the same rows, the same ops — only which chrome carries "back".
public enum PageNavigation {
    /// Push a `NavigationStack` when there is only room for one column. Right
    /// whenever this view is the navigation context — a sheet, a window, a tab
    /// that does not push on its own.
    case stack
    /// Never push a stack. One column keeps the breadcrumb, whose back chevron
    /// pops the model directly, so a host whose scene owns navigation has
    /// exactly one thing that goes back.
    case breadcrumb
}

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
    private let navigation: PageNavigation

    /// The narrow layout's stack, mirroring the trail. Ids, not pages: a stack
    /// element must survive the document changing underneath it, and an id is the
    /// one thing about a page that does.
    @State private var path: [String] = []
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init(
        model: Model,
        theme: FlowerTheme = .default,
        rootLabel: String = "Document",
        navigation: PageNavigation = .stack
    ) {
        self.model = model
        self.theme = theme
        self.rootLabel = rootLabel
        self.navigation = navigation
    }

    public var body: some View {
        GeometryReader { geo in
            if geo.size.width >= twoPaneMinWidth, model.pages.twoPane {
                panes
            } else if navigation == .breadcrumb {
                column
            } else {
                stack
            }
        }
        .onAppear { model.showPages() }
    }

    // ── the narrow layout, for a host that owns navigation ────────────────────

    /// One pane, with the breadcrumb above it.
    ///
    /// The same slide `panes` uses, on the pane that is actually there: a
    /// navigation here is still a step along the trail, and animating it in the
    /// direction it went is the only thing distinguishing deeper from further
    /// out once the rows have changed.
    private var column: some View {
        VStack(spacing: 0) {
            breadcrumb
            Divider()
            sliding(model.pages.page, role: .cursor)
        }
        .animation(reduceMotion ? nil : .easeOut(duration: 0.22),
                   value: model.pages.page.focus)
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
            .accessibilityLabel("Back")
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

/// The fold, at the same seam flower-core offers it (`Page::partitioned`): a
/// stable partition on `demoted`, each entry keeping the index it has in the
/// whole item list — which is what `selected` counts, and what must not shift
/// when the fold rearranges what is *drawn*.
///
/// Stability matters for the same reason core states: demotion is root-scoped,
/// so a group header and the members inlined under it always land in the same
/// run, adjacent and in order — the fold can never cut a group in half.
func partitionDemoted<Item: PageItemDisplaying>(
    _ items: [Item]
) -> (promoted: [(index: Int, item: Item)], demoted: [(index: Int, item: Item)]) {
    var promoted: [(index: Int, item: Item)] = []
    var demoted: [(index: Int, item: Item)] = []
    for (i, item) in items.enumerated() {
        if item.demoted { demoted.append((i, item)) } else { promoted.append((i, item)) }
    }
    return (promoted, demoted)
}

/// What a container's row says about it: its contents when they fit, and a
/// count when they don't.
///
/// `1 field ›` is strictly less than the document says when the field is right
/// there — the count is the fallback for a container too big to put on a row,
/// which is the only case where counting beats showing. Core caps the summary
/// it offers; the row truncates whatever is left over.
func drillSummary<Item: PageItemDisplaying>(_ item: Item) -> String {
    if let summary = item.summary { return summary }
    let n = item.count
    if item.kind == "seq" { return n == 1 ? "1 item" : "\(n) items" }
    return n == 1 ? "1 field" : "\(n) fields"
}

/// What a row announces as its name — the same resolution the drawn name line
/// makes (schema title, then the prettified key, then the key as the document
/// spells it), so VoiceOver and the screen agree about what a row is called.
/// A titled sequence item announces both, index first, like the row shows both.
func rowAccessibilityLabel<Item: PageItemDisplaying>(_ item: Item) -> String {
    let own = item.displayTitle ?? (item.canRename ? prettify(item.label) : item.label)
    guard let title = item.title else { return own }
    return "\(own), \(title)"
}

/// ...and as its value: where it goes, what it counts, or what it holds — the
/// same precedence the trailing edge draws in.
func rowAccessibilityValue<Item: PageItemDisplaying>(_ item: Item) -> String {
    if let link = item.linkLabel { return link }
    if item.role == "drill" { return drillSummary(item) }
    return item.preview.isEmpty ? "Not set" : item.preview
}

/// One page: its items as a card of settings rows, with the demoted ones folded
/// behind an "Advanced" disclosure below it.
private struct PagePane<Model: PageDriving>: View {
    typealias Page = Model.Pages.Page

    let page: Page
    @ObservedObject var model: Model
    let theme: FlowerTheme
    let rootLabel: String
    let role: PaneRole

    /// Whether the reader opened the fold. Per-pane and reset by navigation
    /// (the pane is identity-keyed on its focus), which is the disclosure's
    /// ordinary lifetime: "advanced" is a default about arriving, not a mode.
    @State private var advancedOpened = false

    var body: some View {
        let split = partitionDemoted(page.items)
        // Demotion says these rows sit below the ones a reader came to edit —
        // so the fold only exists where there are both kinds. A page of nothing
        // but demoted rows has nothing to protect them from, and a page *under*
        // a demoted key was opened on purpose (core marks the whole page, so
        // every run would be the folded one and the page would arrive shut).
        let folded = !page.demoted && !split.promoted.isEmpty && !split.demoted.isEmpty
        ScrollView {
            VStack(alignment: .leading, spacing: 7) {
                if role != .cursor {
                    Text(paneTitle.uppercased())
                        .font(.system(size: 11, weight: .semibold))
                        .tracking(0.6)
                        .foregroundStyle(.tertiary)
                        .padding(.leading, 14)
                        .accessibilityAddTraits(.isHeader)
                }
                if page.items.isEmpty {
                    Text("Empty")
                        .font(.system(size: 14))
                        .foregroundStyle(.tertiary)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 12)
                } else if folded {
                    card(split.promoted)
                    advancedHeader(count: split.demoted.count,
                                   open: advancedOpen(split.demoted))
                    if advancedOpen(split.demoted) {
                        card(split.demoted)
                    }
                } else {
                    card(split.promoted + split.demoted)
                }
                if role == .cursor, model.canAddChild(pageId: page.focus) {
                    AddChildRow(pageId: page.focus, model: model)
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 14)
            .frame(maxWidth: 560, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .opacity(role == .preview ? 0.55 : 1)
        .allowsHitTesting(role.isInteractive)
        .accessibilityHidden(role == .preview)
    }

    /// Whether the fold is showing: opened by hand, or held open by what must
    /// not disappear into it — a disclosure that could hide the row being
    /// edited would make the fold a place where state goes to get lost.
    ///
    /// The cursor is deliberately *not* on that list for the pane being looked
    /// at: a selection often arrives carried over from another projection (the
    /// tree's cursor sits on the first row, which is frequently a demoted one),
    /// and prying the fold open for it would defeat the fold on arrival. On the
    /// trail pane the marked row is the way back — that one stays visible.
    private func advancedOpen(_ demoted: [(index: Int, item: Page.Item)]) -> Bool {
        advancedOpened || demoted.contains { entry in
            model.editingId == entry.item.id
                || model.renamingId == entry.item.id
                || (role != .cursor && (page.selected.map { Int($0) == entry.index } ?? false))
        }
    }

    /// The fold's own row: what it is called, and — while shut — how much it
    /// holds, so closed never reads as empty.
    private func advancedHeader(count: Int, open: Bool) -> some View {
        Button {
            advancedOpened.toggle()
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "chevron.right")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(.tertiary)
                    .rotationEffect(open ? .degrees(90) : .zero)
                Text("ADVANCED")
                    .font(.system(size: 11, weight: .semibold))
                    .tracking(0.5)
                    .foregroundStyle(.secondary)
                if !open {
                    Text("\(count)")
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                }
                Spacer()
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 14)
        .padding(.top, 9)
        .accessibilityIdentifier("flower-advanced")
        .accessibilityLabel("Advanced")
        .accessibilityValue(open ? "expanded, \(count) fields" : "collapsed, \(count) fields")
    }

    private var paneTitle: String {
        guard let last = page.crumbs.last else { return rootLabel }
        return prettify(last.label)
    }

    private func card(_ entries: [(index: Int, item: Page.Item)]) -> some View {
        VStack(spacing: 0) {
            ForEach(Array(entries.enumerated()), id: \.element.item.id) { pos, entry in
                if entry.item.role == "group" {
                    GroupHeaderRow(item: entry.item, first: pos == 0)
                } else {
                    if pos > 0, entries[pos - 1].item.role != "group" {
                        Divider().padding(.leading, 57 + CGFloat(entry.item.inset) * 16)
                    }
                    PageRow(item: entry.item, model: model, theme: theme,
                            selected: page.selected.map { Int($0) == entry.index } ?? false,
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

/// The page's own add affordance: a row below the cards, present only where the
/// host said this page's container takes one (``PageDriving/canAddChild(pageId:)``).
///
/// The declared-but-absent fields come first — they are what this document's
/// schema expects, and without an offer they are unreachable, since rows come
/// from the document and a field with no value has no row. A field whose
/// vocabulary is closed opens as a submenu of its terms, because it has to
/// arrive holding a legal one. A custom key stays available underneath for
/// anything undeclared.
private struct AddChildRow<Model: PageDriving>: View {
    let pageId: String
    @ObservedObject var model: Model

    var body: some View {
        Menu {
            let declared = model.addableChildren(of: pageId)
            if !declared.isEmpty {
                Section("Declared") {
                    ForEach(declared) { field in
                        if field.terms.isEmpty {
                            Button {
                                model.pageAddChild(id: pageId, key: field.key, value: "")
                            } label: {
                                label(for: field)
                            }
                            .help(field.description ?? "")
                        } else {
                            Menu {
                                ForEach(field.terms, id: \.self) { term in
                                    Button(term) {
                                        model.pageAddChild(id: pageId, key: field.key, value: term)
                                    }
                                }
                            } label: {
                                label(for: field)
                            }
                        }
                    }
                }
            }
            Button {
                model.pageAddChild(id: pageId)
            } label: {
                Label("Custom Field…", systemImage: "character.cursor.ibeam")
            }
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "plus")
                    .font(.system(size: 12, weight: .medium))
                Text("Add a Field…")
                    .font(.system(size: 13))
            }
            .foregroundStyle(Color.accentColor)
            .contentShape(Rectangle())
        }
        .menuIndicator(.hidden)
        .fixedSize()
        #if os(macOS)
        .menuStyle(.borderlessButton)
        #endif
        .padding(.horizontal, 14)
        .padding(.top, 2)
        .accessibilityIdentifier("flower-add-field")
        .accessibilityLabel("Add a Field")
        .help("Add a field to this page")
    }

    /// The offer, presented as the row it would become: the schema's name and
    /// symbol where it gave them, the same inference the row would fall back on
    /// where it did not.
    private func label(for field: AddableChild) -> some View {
        let symbol = field.icon.flatMap(FlowerPalette.symbol(forSemanticIcon:))
            ?? FlowerPalette.inferredIcon(label: field.key, kind: field.kind ?? "str").symbol
        return Label(field.title ?? prettify(field.key), systemImage: symbol)
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
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(name)
        .accessibilityAddTraits(.isHeader)
    }

    private var name: String {
        let own = item.displayTitle ?? prettify(item.label)
        guard let title = item.title else { return own }
        return "\(own) · \(title)"
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

    /// Whether the row's value is an interactive control of its own — a
    /// toggle, a vocabulary menu, an open editor. Those rows stay AX containers
    /// so the control keeps its role and its own label; every other row
    /// collapses to one element, because a tap gesture on an `HStack` is
    /// nothing to accessibility: no role, no name, no press action, and a page
    /// of them reads as a page of nothing.
    private var hasOwnControl: Bool {
        if isRenaming { return true }
        if isDrill || item.linkLabel != nil || item.isReadonly { return false }
        return !item.enumOptions.isEmpty || item.kind == "bool" || isEditing
    }

    @ViewBuilder var body: some View {
        if hasOwnControl {
            core
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier(item.id)
        } else {
            // One element, named and valued the way the row draws, whose press
            // is the tap's `pageActivate` — and the context menu again as
            // custom actions, which is the only way its operations reach
            // VoiceOver at all.
            core
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(rowAccessibilityLabel(item))
                .accessibilityValue(rowAccessibilityValue(item))
                .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
                .accessibilityHint(axHint)
                .accessibilityIdentifier(item.id)
                .accessibilityAction { model.pageActivate(item) }
                .accessibilityActions { PageRowMenu(item: item, model: model) }
        }
    }

    /// Spoken after the value, saying what acting on the row does — or, for a
    /// maintained field, why nothing does.
    private var axHint: String {
        if let description = item.description, !description.isEmpty { return description }
        if let link = item.linkLabel { return "Opens \(link)" }
        if item.isReadonly { return "Maintained automatically" }
        return ""
    }

    private var core: some View {
        HStack(spacing: 12) {
            IconTile(label: item.label, kind: item.kind, icon: item.icon, tint: item.tint)
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

    /// What names the row, and — where a schema said one — the sentence under it.
    ///
    /// A titled sequence item keeps its index *and* gains the title: the index is
    /// what the path addresses and what a reorder moves, so dropping it would
    /// leave nothing to reconcile the row with the document — but it is dimmed,
    /// because on a list of twenty steps the title is what you are reading and
    /// the index is what you check afterwards.
    @ViewBuilder private var name: some View {
        if isRenaming {
            TextField("key", text: $model.renameBuffer)
                .textFieldStyle(.plain)
                .font(.system(size: 15, weight: .medium))
                .accessibilityLabel("Rename \(item.label)")
                .focused($keyFocused)
                .onSubmit { model.commitRename() }
                #if os(macOS)
                .onExitCommand { model.cancelRename() }
                #endif
                .onAppear { keyFocused = true }
                .frame(maxWidth: 180)
        } else {
            VStack(alignment: .leading, spacing: 1) {
                nameLine
                // The schema's help text, where it has some. One line: a row is
                // a row, and a paragraph under one of them would turn a list you
                // scan into a page you read. The full sentence is the tooltip.
                if let description = item.description, !description.isEmpty {
                    Text(description)
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
            }
            .help(item.description ?? "")
        }
    }

    @ViewBuilder private var nameLine: some View {
        if let title = item.title {
            HStack(spacing: 6) {
                Text(item.label)
                    .font(.system(size: 13, design: .monospaced))
                    .foregroundStyle(.tertiary)
                Text(title)
                    .font(.system(size: 15, weight: .medium))
                    .lineLimit(1)
            }
        } else {
            // The schema's name, then the key title-cased, then the key as it is.
            // The last step is not a fallback so much as a rule: a key that
            // cannot be renamed is a sequence index or a value the document
            // spells exactly one way, and prettifying it would name the row
            // something the document does not contain.
            Text(item.displayTitle ?? (item.canRename ? prettify(item.label) : item.label))
                .font(.system(size: 15))
                .lineLimit(1)
        }
    }

    @ViewBuilder private var trailing: some View {
        if isDrill {
            HStack(spacing: 6) {
                Text(drillSummary(item))
                    .font(.system(size: 13, design: item.summary != nil ? .monospaced : .default))
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Image(systemName: "chevron.right")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.tertiary)
            }
        } else if let link = item.linkLabel {
            // A reference the host resolved. The storage form is for machines,
            // so the row shows where the value *goes* — and drawing it ahead of
            // `isReadonly` is the point: a relation some other surface owns is
            // usually maintained too, and the lock would say "nothing for you
            // here" about the most navigable row on the page.
            HStack(spacing: 5) {
                Text(link)
                    .font(.system(size: 15))
                    .foregroundStyle(Color.accentColor)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Image(systemName: "chevron.right")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.tertiary)
                    .accessibilityHidden(true)
            }
        } else if item.isReadonly {
            // Maintained by something other than the reader. Shown, because a
            // field visible in the file and absent from the editor reads as data
            // loss — but with no control, because the only honest thing a
            // control could do here is be overwritten by the next save.
            HStack(spacing: 5) {
                Text(item.preview.isEmpty ? "Not set" : item.preview)
                    .font(.system(size: 15, design: valueDesign))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Image(systemName: "lock")
                    .font(.system(size: 11))
                    .foregroundStyle(.tertiary)
                    .accessibilityHidden(true)
            }
        } else if !item.enumOptions.isEmpty {
            // A vocabulary → a list of it. Ahead of `isEditing` on purpose: this
            // control owns its own text-entry state for the open case, so a stray
            // `beginEdit` cannot strand a text box on top of a field whose legal
            // values are a list.
            ChoiceControl(item: item, model: model)
        } else if item.kind == "bool" {
            Toggle("", isOn: Binding(get: { item.preview == "true" },
                                     set: { model.setBool(item, $0) }))
                .labelsHidden()
                .toggleStyle(.switch)
                .accessibilityLabel(rowAccessibilityLabel(item))
        } else if isEditing {
            HStack(spacing: 6) {
                TextField("value", text: $model.editBuffer)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(size: 14, design: numeric ? .monospaced : .default))
                    .frame(maxWidth: 150)
                    .accessibilityLabel(rowAccessibilityLabel(item))
                    .focused($focused)
                    .onSubmit { model.commitEdit() }
                    #if os(macOS)
                    .onExitCommand { model.cancelEdit() }
                    #endif
                    .onAppear { focused = true }
                if numeric {
                    Stepper("", onIncrement: { step(+1) }, onDecrement: { step(-1) })
                        .labelsHidden()
                        .accessibilityLabel("Adjust \(rowAccessibilityLabel(item))")
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

/// A scalar whose schema names the terms it may take.
///
/// A text box over a controlled field is a small lie: it accepts every string,
/// and the ones the vocabulary does not list are refused somewhere the typist
/// cannot see — on save, or silently, by whatever reads the document later. A
/// list is the same information told in advance.
///
/// Two vocabularies, two renderings, because they promise different things
/// (``PageItemDisplaying/isClosedEnum``). A **closed** one is the whole of what
/// is legal, so the list is the whole of the control. An **open** one is the
/// common answers over a wider legal space, so the list is a shortcut and
/// "Other…" is the rest of it — offering only the terms there would hide values
/// that are perfectly valid, which is a worse failure than the text box was.
///
/// It holds the open case's typing itself rather than deferring to the row,
/// which is why it sits ahead of the row's `isEditing` branch: the two states
/// are exclusive here and keeping them in one view is what makes that true by
/// construction.
private struct ChoiceControl<Model: PageDriving>: View {
    typealias Item = Model.Pages.Page.Item

    let item: Item
    @ObservedObject var model: Model
    @FocusState private var focused: Bool

    /// The document holds something the vocabulary does not list. Not
    /// automatically wrong — an open vocabulary is *made* of this case, and a
    /// closed one still carries values from before a term was retired.
    private var isUnlisted: Bool {
        !item.preview.isEmpty && !item.enumOptions.contains(item.preview)
    }

    /// A value this field is not allowed to hold: unlisted, under a vocabulary
    /// that admits nothing else.
    ///
    /// Said rather than corrected. Snapping it to a legal term would edit the
    /// document on the reader's behalf over something they may not have written
    /// and cannot now see, and quietly rendering it as if it were fine is how it
    /// survives to the next reader. The row shows the value it really has, and
    /// marks it.
    private var isIllegal: Bool { item.isClosedEnum && isUnlisted }

    var body: some View {
        if model.editingId == item.id {
            TextField("value", text: $model.editBuffer)
                .textFieldStyle(.roundedBorder)
                .font(.system(size: 14))
                .frame(maxWidth: 150)
                .accessibilityLabel(item.displayTitle ?? prettify(item.label))
                .focused($focused)
                .onSubmit { model.commitEdit() }
                #if os(macOS)
                .onExitCommand { model.cancelEdit() }
                #endif
                .onAppear { focused = true }
        } else {
            menu
        }
    }

    private var menu: some View {
        Menu {
            ForEach(item.enumOptions, id: \.self) { term in
                Button {
                    model.setChoice(item, term)
                } label: {
                    // `Label` rather than a checkmark column: the menu lays the
                    // glyph out itself, so the terms stay aligned whether or not
                    // one of them is current.
                    if term == item.preview {
                        Label(term, systemImage: "checkmark")
                    } else {
                        Text(term)
                    }
                }
            }
            if !item.isClosedEnum {
                Divider()
                Button("Other…") { model.beginEdit(item) }
            }
        } label: {
            HStack(spacing: 5) {
                if isIllegal {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(.system(size: 11))
                        .foregroundStyle(.orange)
                        .accessibilityHidden(true)
                }
                Text(item.preview.isEmpty ? "Not set" : item.preview)
                    .font(.system(size: 15))
                    .foregroundStyle(labelColor)
                    .lineLimit(1)
                Image(systemName: "chevron.up.chevron.down")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(.tertiary)
                    .accessibilityHidden(true)
            }
        }
        .menuIndicator(.hidden)
        .fixedSize()
        #if os(macOS)
        .menuStyle(.borderlessButton)
        #endif
        // The name VoiceOver reads is the name the row shows — the schema's,
        // where it named one. A picker announced by its raw key while the row
        // beside it reads "Content Format" is two names for one control.
        .accessibilityLabel(item.displayTitle ?? prettify(item.label))
        .accessibilityValue(accessibilityValue)
    }

    private var labelColor: Color {
        if isIllegal { return .orange }
        return item.preview.isEmpty ? Color.gray.opacity(0.8) : FlowerPalette.value(forKind: item.kind)
    }

    /// Spoken as well as drawn — the warning triangle is the only thing marking
    /// an illegal value, and a glyph nothing announces is not a warning.
    private var accessibilityValue: String {
        let value = item.preview.isEmpty ? "Not set" : item.preview
        return isIllegal ? "\(value) — not one of the allowed values" : value
    }
}

/// The per-row menu. Every operation takes a path, and a group header still
/// carries one, so a group inlined into this page is as operable as a row that
/// opens its own — inlining is a presentation default, never a cage.
private struct PageRowMenu<Model: PageDriving>: View {
    typealias Item = Model.Pages.Page.Item

    let item: Item
    @ObservedObject var model: Model

    /// Whether a *value* can be typed here.
    ///
    /// Not for a maintained field, and not for a closed vocabulary either: the
    /// row's list is already the whole of what that field may hold, so a free-text
    /// escape beside it would offer to write the one thing the schema refuses.
    /// An open vocabulary keeps it — there, an unlisted value is legal, and the
    /// row's own "Other…" is this same intent.
    private var canTypeValue: Bool {
        item.role == "scalar" && !item.isReadonly && !item.isClosedEnum
    }

    var body: some View {
        // The same intent the tap sends — a link row's default action is going,
        // and the menu names what tapping does rather than offering a second way.
        if item.linkLabel != nil {
            Button("Follow") { model.pageActivate(item) }
        }
        if canTypeValue {
            Button("Edit Value") { model.beginEdit(item) }
        }
        if item.role == "drill" {
            Button("Open") { model.pageOpen(id: item.id) }
        }
        if item.canRename, !item.isReadonly {
            Button("Rename") { model.beginRename(item) }
        }
        if model.canAddChild(item) {
            Button(item.kind == "seq" ? "Add Item" : "Add Field") {
                model.pageAddChild(id: item.id)
            }
        }
        // Reordering and deleting stay off a maintained row for the same reason
        // its editor does: whatever maintains it decides where it sits and
        // whether it exists, so both would be undone by the next write.
        if !item.isReadonly {
            Divider()
            Button("Move Up") { model.moveItemUp(item) }
            Button("Move Down") { model.moveItemDown(item) }
            Divider()
            Button("Delete", role: .destructive) { model.delete(item) }
        }
    }
}
