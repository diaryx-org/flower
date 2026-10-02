//  FlowerPageView.swift
//
//  The page face of the editor: one container at a time, pushed and popped —
//  flower-core's `page` projection, rendered the way a settings app renders a
//  preference pane.
//
//  It is the only face. Showing the whole document at once used to be a second
//  surface; now it is the same one under a generous inline budget (the model's
//  `setInlineBudget`), which inlines whole subtrees onto their parent's page —
//  a small document renders on one page, and a deep one pays for depth with a
//  navigation step instead of a column of indentation. How much inlines is a
//  question about the document (and the room), not about what the user is
//  allowed to do: every edit is path-addressed and lossless at any budget.
//
//  What the projection ships is a flat item list — insets, group headers, a
//  demoted flag — and what a settings screen draws is structure: cards under
//  captions, a tag list as chips, an "Advanced" fold. `pageLayout` is the seam
//  between them (see its docs), pure and testable, so the two hosts of this
//  view lay a page out identically without sharing a pixel of chrome.
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
import UniformTypeIdentifiers

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
        // A refusal is felt as well as read — and is the only sign of one that
        // has no row on screen to be drawn under, like "nothing to undo".
        .onChange(of: model.notice?.seq) { _ in
            if model.notice?.kind == .rejected { signalRefusal() }
        }
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

// ── Laying a run of items out as a settings screen ────────────────────────────

/// One drawable line of a card: a row, or a scalar sequence folded into a
/// single line of chips.
///
/// `inset` is the *drawn* indentation, not the projection's: a titled section
/// already says its members belong to it with a caption, so they are rebased a
/// rank left rather than indented under a name that is no longer a row.
enum PageEntry<Item: PageItemDisplaying>: Identifiable {
    /// One item as one row. `index` is the item's position in the whole item
    /// list — what `selected` counts.
    case row(index: Int, item: Item, inset: Int)
    /// A scalar sequence as one row of chips: the header names it, the members
    /// are the chips. The one place several items share a line.
    case chips(index: Int, header: Item, members: [(index: Int, item: Item)], inset: Int)

    var id: String {
        switch self {
        case let .row(_, item, _): return item.id
        case let .chips(_, header, _, _): return header.id
        }
    }

    /// Whether this entry draws as a caption rather than a row — the hairline a
    /// caption carries stands in for the divider its neighbour would get.
    var isGroupCaption: Bool {
        if case let .row(_, item, _) = self { return item.role == "group" }
        return false
    }
}

/// One card of a pane: a run of entries, under the group that titles it — or
/// under no title, for a run of the page's own rows.
struct PageSection<Item: PageItemDisplaying>: Identifiable {
    let id: String
    /// The rank-0 group whose members this card holds, drawn as a caption above
    /// it rather than as a row inside it. `nil` for an untitled run.
    let header: (index: Int, item: Item)?
    let entries: [PageEntry<Item>]
}

/// Whether a group renders as one row of chips: a scalar sequence, all of whose
/// members sit directly under it. The settings-screen rendering of a tag list —
/// `[0]`, `[1]`, `[2]` are positions, not information, and a page has no reason
/// to spend a row on each.
private func chipsEligible<Item: PageItemDisplaying>(
    _ header: Item, _ members: [(index: Int, item: Item)]
) -> Bool {
    header.kind == "seq" && !members.isEmpty
        && members.allSatisfy { $0.item.role == "scalar" && $0.item.inset == header.inset + 1 }
}

/// A run of a page's items, laid out the way a settings screen reads.
///
/// Three rules, applied to the projection's flat, inset-tagged list:
///
/// - a **rank-0 group** becomes its own card, its name the caption above it and
///   its members rebased a rank left — the grammar a grouped settings screen
///   uses for its sections;
/// - a **scalar sequence** becomes one row of chips wherever it sits, because a
///   list of tags is one fact about the document, not `n` rows of positions;
/// - everything else stays a row at its inset, group headers included — a
///   nested group is a caption *inside* its section's card, which is as far as
///   the eye should be asked to track containment before the budget hands the
///   subtree a page of its own.
///
/// Runs of rank-0 rows between titled sections collect into untitled cards, in
/// document order throughout. Pure and total, so both layouts (and the tests)
/// lay a page out identically.
func pageLayout<Item: PageItemDisplaying>(
    _ entries: [(index: Int, item: Item)]
) -> [PageSection<Item>] {
    var sections: [PageSection<Item>] = []
    var run: [(index: Int, item: Item)] = []
    func flushRun() {
        guard !run.isEmpty else { return }
        sections.append(
            PageSection(id: "run-\(run[0].item.id)", header: nil,
                        entries: cardEntries(run, shift: 0)))
        run = []
    }
    var i = 0
    while i < entries.count {
        let entry = entries[i]
        if entry.item.role == "group", entry.item.inset == 0 {
            var j = i + 1
            var members: [(index: Int, item: Item)] = []
            while j < entries.count, entries[j].item.inset > 0 {
                members.append(entries[j])
                j += 1
            }
            if chipsEligible(entry.item, members) {
                // A chips row is a row: it stays in the surrounding card.
                run.append(entry)
                run.append(contentsOf: members)
            } else {
                flushRun()
                sections.append(
                    PageSection(id: entry.item.id, header: (entry.index, entry.item),
                                entries: cardEntries(members, shift: 1)))
            }
            i = j
        } else {
            run.append(entry)
            i += 1
        }
    }
    flushRun()
    return sections
}

/// One card's worth of entries: chips folded, every drawn inset shifted left by
/// `shift` (1 inside a titled section, whose caption already says what the
/// members belong to).
private func cardEntries<Item: PageItemDisplaying>(
    _ entries: [(index: Int, item: Item)], shift: Int
) -> [PageEntry<Item>] {
    var out: [PageEntry<Item>] = []
    var i = 0
    while i < entries.count {
        let entry = entries[i]
        let inset = Int(entry.item.inset) - shift
        if entry.item.role == "group" {
            var j = i + 1
            var members: [(index: Int, item: Item)] = []
            while j < entries.count, entries[j].item.inset > entry.item.inset {
                members.append(entries[j])
                j += 1
            }
            if chipsEligible(entry.item, members) {
                out.append(.chips(index: entry.index, header: entry.item,
                                  members: members, inset: inset))
                i = j
                continue
            }
        }
        out.append(.row(index: entry.index, item: entry.item, inset: inset))
        i += 1
    }
    return out
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

/// The rows of a page whose keys are data rather than field names, by id.
///
/// No schema says which maps are records and which are dictionaries, so this
/// reads it off the document: a nested map holding nothing but plain strings
/// under keys no schema named — `scripts`, `dependencies`, `env`, an alias
/// table — is keyed by names somebody chose. You type `bun run dev`, not
/// `bun run Dev`, so title-casing `dev` would show a key the document does not
/// contain. The document root is never one; it is the record everything else
/// hangs off.
///
/// When it guesses wrong it fails toward the truth: a small record of strings
/// shows its keys as written, which is less polished but never wrong.
func verbatimKeyIds<Item: PageItemDisplaying>(focus: String, items: [Item]) -> Set<String> {
    var children: [String: [Item]] = [:]
    for (item, parent) in zip(items, parentIds(focus: focus, items: items)) {
        children[parent, default: []].append(item)
    }
    var out: Set<String> = []
    for (parent, members) in children where !parent.isEmpty {
        let dictionary = members.allSatisfy {
            $0.role == "scalar" && $0.kind == "str" && $0.canRename
                && $0.displayTitle == nil && $0.enumOptions.isEmpty
        }
        if dictionary { out.formUnion(members.map(\.id)) }
    }
    return out
}

/// Each item's parent, by id, in item order: the nearest earlier item one inset
/// shallower, or the page's own container (`focus`) for its top-level rows.
/// The projection ships a flat list, and this is the containment it flattened.
func parentIds<Item: PageItemDisplaying>(focus: String, items: [Item]) -> [String] {
    var lastAt: [Int: String] = [:]
    return items.map { item in
        let depth = Int(item.inset)
        let parent = depth == 0 ? focus : (lastAt[depth - 1] ?? focus)
        lastAt[depth] = item.id
        return parent
    }
}

/// How many places dropping `dragged` onto `target` moves it among its
/// siblings — negative is earlier — or `nil` when the drop is not a reorder.
///
/// A row can only take the place of a sibling, because a move is a reorder
/// within one container: dropping a script into the dependencies would be
/// moving a key to another map, which is a different edit with its own
/// questions (what if the key is taken there?) and not one a drag should
/// answer by accident.
func reorderOffset<Item: PageItemDisplaying>(
    moving dragged: String, onto target: String, focus: String, items: [Item]
) -> Int? {
    let parents = parentIds(focus: focus, items: items)
    guard let from = items.firstIndex(where: { $0.id == dragged }),
          let to = items.firstIndex(where: { $0.id == target }),
          parents[from] == parents[to] else { return nil }
    let siblings = items.indices.filter { parents[$0] == parents[from] }
    guard let a = siblings.firstIndex(of: from), let b = siblings.firstIndex(of: to) else { return nil }
    return b - a
}

/// What a row is called: the schema's title, then the key title-cased, then
/// the key as it is. The last step is not a fallback so much as a rule: a key
/// that cannot be renamed is a sequence index or a value the document spells
/// exactly one way, and a `verbatim` key is a name somebody chose — either way,
/// prettifying it would name the row something the document does not contain.
func rowName<Item: PageItemDisplaying>(_ item: Item, verbatim: Bool = false) -> String {
    item.displayTitle ?? (item.canRename && !verbatim ? prettify(item.label) : item.label)
}

/// Whether a card's rows wear icon tiles at all: only when one of them says
/// something by it. A card where every tile would be its value kind's fallback
/// — the "Aa" of a card of strings — draws none, and gives the names the room.
/// When any row's tile is distinctive, every row keeps one so the names stay
/// in a column.
func cardShowsTiles<Item: PageItemDisplaying>(
    _ entries: [PageEntry<Item>], verbatim: Set<String> = []
) -> Bool {
    func distinctive(_ item: Item) -> Bool {
        // A chosen name is not a field name, and reading an icon off it would
        // give a dependency called `url-parse` a globe.
        !verbatim.contains(item.id)
            && FlowerPalette.isDistinctive(label: item.label, icon: item.icon, tint: item.tint)
    }
    return entries.contains { entry in
        switch entry {
        case let .row(_, item, _): return item.role != "group" && distinctive(item)
        case let .chips(_, header, _, _): return distinctive(header)
        }
    }
}

/// What a row announces as its name — the same resolution the drawn name line
/// makes (``rowName(_:verbatim:)``), so VoiceOver and the screen agree about
/// what a row is called. A titled sequence item announces both, index first,
/// like the row shows both.
func rowAccessibilityLabel<Item: PageItemDisplaying>(_ item: Item, verbatim: Bool = false) -> String {
    let own = rowName(item, verbatim: verbatim)
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

    /// The row being dragged, from this pane — the drag's own record of what it
    /// carries, since the pasteboard only says so asynchronously.
    @State private var dragging: String?
    /// Where the dragged row would land: the edge of the row it would take the
    /// place of.
    @State private var dropIndicator: RowDropIndicator?

    var body: some View {
        let split = partitionDemoted(page.items)
        // Demotion says these rows sit below the ones a reader came to edit —
        // so the fold only exists where there are both kinds. A page of nothing
        // but demoted rows has nothing to protect them from, and a page *under*
        // a demoted key was opened on purpose (core marks the whole page, so
        // every run would be the folded one and the page would arrive shut).
        let folded = !page.demoted && !split.promoted.isEmpty && !split.demoted.isEmpty
        ScrollView {
            // Lazy, because a generous inline budget can put a whole document
            // on this one page — the case the second surface used to carry.
            LazyVStack(alignment: .leading, spacing: 14) {
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
                    addRow
                } else if folded {
                    sections(pageLayout(split.promoted))
                    // Above the fold, not under it: adding a field belongs to
                    // the rows a reader came to edit, and a fold that has to be
                    // opened to reach the offer — or that pushes it a screen
                    // down when opened — hides an action that is not advanced.
                    addRow
                    advancedHeader(count: split.demoted.count,
                                   open: advancedOpen(split.demoted))
                    if advancedOpen(split.demoted) {
                        sections(pageLayout(split.demoted))
                    }
                } else {
                    sections(pageLayout(split.promoted + split.demoted))
                    addRow
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

    /// The cards of one run: each titled section under its caption, each
    /// untitled run as a bare card — the grammar of a grouped settings screen.
    private func sections(_ sections: [PageSection<Page.Item>]) -> some View {
        ForEach(sections) { section in
            VStack(alignment: .leading, spacing: 7) {
                if let header = section.header {
                    SectionCaption(item: header.item, model: model,
                                   selected: page.selected.map { Int($0) == header.index } ?? false)
                }
                card(section.entries)
            }
        }
    }

    /// The offer to add a child, where the page takes one and the pane is the
    /// one being edited. A preview or a trail pane is a picture of somewhere
    /// else; an action drawn on it would act on a page the reader has left.
    @ViewBuilder private var addRow: some View {
        if role == .cursor, model.canAddChild(pageId: page.focus) {
            AddChildRow(pageId: page.focus, model: model)
        }
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

    private func selected(_ index: Int) -> Bool {
        page.selected.map { Int($0) == index } ?? false
    }

    private func card(_ entries: [PageEntry<Page.Item>]) -> some View {
        let verbatim = verbatimKeyIds(focus: page.focus, items: page.items)
        let tiles = cardShowsTiles(entries, verbatim: verbatim)
        let dividerLead: CGFloat = tiles ? 57 : 14
        return VStack(spacing: 0) {
            ForEach(Array(entries.enumerated()), id: \.element.id) { pos, entry in
                switch entry {
                case let .row(index, item, inset):
                    if item.role == "group" {
                        GroupHeaderRow(item: item, model: model, inset: inset, first: pos == 0)
                    } else {
                        if pos > 0, !entries[pos - 1].isGroupCaption {
                            Divider().padding(.leading, dividerLead + CGFloat(inset) * 16)
                        }
                        reorderable(
                            PageRow(item: item, model: model, theme: theme,
                                    selected: selected(index), inset: inset, role: role,
                                    verbatimKey: verbatim.contains(item.id), showsTile: tiles),
                            item: item, lead: dividerLead + CGFloat(inset) * 16)
                    }
                case let .chips(index, header, members, inset):
                    if pos > 0, !entries[pos - 1].isGroupCaption {
                        Divider().padding(.leading, dividerLead + CGFloat(inset) * 16)
                    }
                    reorderable(
                        PageChipsRow(header: header, members: members, model: model,
                                     selected: selected(index)
                                         || members.contains { selected($0.index) },
                                     inset: inset, role: role, showsTile: tiles),
                        item: header, lead: dividerLead + CGFloat(inset) * 16)
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

    /// A row that can be dragged onto a sibling to take its place.
    ///
    /// The row itself is the handle: a Mac list reorders by dragging the row,
    /// and an iPhone by pressing and holding it, so neither needs a grip drawn
    /// on every row of a page for something done now and then. The drag only
    /// starts once the pointer moves, so a click still edits.
    ///
    /// Only on the pane being edited, and never on a row that is being typed
    /// in — its text field owns the pointer — or on one something else
    /// maintains, which decides where it sits.
    @ViewBuilder private func reorderable<Row: View>(
        _ row: Row, item: Page.Item, lead: CGFloat
    ) -> some View {
        if role == .cursor {
            let delegate = RowDropDelegate(
                offset: { [page] in
                    dragging.flatMap { reorderOffset(moving: $0, onto: item.id,
                                                     focus: page.focus, items: page.items) }
                },
                target: item.id,
                indicator: $dropIndicator,
                accept: { token in
                    guard let id = dragging, token == RowDropDelegate.token(id) else { return }
                    dragging = nil
                    // Resolved against the frame at the drop, not the one the
                    // drag began in: the model may have moved on since.
                    let page = model.pages.page
                    guard let moved = page.items.first(where: { $0.id == id }),
                          let offset = reorderOffset(moving: id, onto: item.id,
                                                     focus: page.focus, items: page.items),
                          offset != 0 else { return }
                    model.moveItem(moved, by: offset)
                })
            let busy = model.editingId == item.id || model.renamingId == item.id
            Group {
                if busy || item.isReadonly {
                    row
                } else {
                    row.onDrag {
                        dragging = item.id
                        return NSItemProvider(object: RowDropDelegate.token(item.id) as NSString)
                    }
                }
            }
            .onDrop(of: [.plainText], delegate: delegate)
            .overlay(alignment: dropIndicator?.edge == .top ? .top : .bottom) {
                if dropIndicator?.id == item.id {
                    Rectangle()
                        .fill(Color.accentColor)
                        .frame(height: 2)
                        .padding(.leading, lead)
                        .allowsHitTesting(false)
                }
            }
        } else {
            row
        }
    }

    private var cardBackground: some View {
        #if canImport(UIKit)
        Color(.secondarySystemGroupedBackground)
        #else
        Color(nsColor: .controlBackgroundColor)
        #endif
    }
}

/// Where a dragged row would land: the row whose place it would take, and the
/// edge it would arrive at — above a row it is moving up past, below one it is
/// moving down past.
struct RowDropIndicator: Equatable {
    let id: String
    let edge: VerticalEdge
}

/// One row's half of a reorder: whether the drag over it is a legal move, the
/// line that says where it would land, and the move itself on drop.
private struct RowDropDelegate: DropDelegate {
    /// How far the drag in flight would move, or `nil` when it is not one of
    /// this pane's rows or not a sibling of this one.
    let offset: () -> Int?
    let target: String
    @Binding var indicator: RowDropIndicator?
    /// Called with what the drop carried, to move the row if it is the one this
    /// pane is dragging.
    let accept: (String) -> Void

    /// What a dragged row puts on the pasteboard. Checked on drop, so a drag of
    /// ordinary text from elsewhere — or one this pane lost track of — moves
    /// nothing.
    static func token(_ id: String) -> String { "flower-row:\(id)" }

    private var move: Int? { offset().flatMap { $0 == 0 ? nil : $0 } }

    func validateDrop(info: DropInfo) -> Bool { move != nil }

    func dropEntered(info: DropInfo) {
        guard let move else { return }
        indicator = RowDropIndicator(id: target, edge: move < 0 ? .top : .bottom)
    }

    func dropUpdated(info: DropInfo) -> DropProposal? {
        DropProposal(operation: move == nil ? .forbidden : .move)
    }

    func dropExited(info: DropInfo) {
        if indicator?.id == target { indicator = nil }
    }

    func performDrop(info: DropInfo) -> Bool {
        indicator = nil
        guard move != nil, let provider = info.itemProviders(for: [.plainText]).first else {
            return false
        }
        _ = provider.loadObject(ofClass: NSString.self) { object, _ in
            guard let token = object as? String else { return }
            DispatchQueue.main.async { accept(token) }
        }
        return true
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

/// What names a group or section: the schema's title, the prettified key, and —
/// for a titled sequence item — the title that names it.
private func groupName<Item: PageItemDisplaying>(_ item: Item) -> String {
    let own = item.displayTitle ?? prettify(item.label)
    guard let title = item.title else { return own }
    return "\(own) · \(title)"
}

/// The caption above a titled section's card: the rank-0 group whose members
/// the card holds, named but not listed — being the card's title *is* its
/// rendering. It keeps the group's operations in its context menu, because a
/// container that stopped being a row should not stop being deletable.
private struct SectionCaption<Model: PageDriving>: View {
    let item: Model.Pages.Page.Item
    @ObservedObject var model: Model
    let selected: Bool

    var body: some View {
        Text(groupName(item).uppercased())
            .font(.system(size: 11, weight: .semibold))
            .tracking(0.6)
            .foregroundStyle(selected ? Color.accentColor : Color.secondary)
            .padding(.leading, 14)
            .contentShape(Rectangle())
            .onTapGesture { model.pageActivate(item) }
            .contextMenu { PageRowMenu(item: item, model: model) }
            .accessibilityLabel(groupName(item))
            .accessibilityAddTraits(.isHeader)
            .accessibilityIdentifier(item.id)
    }
}

/// The header of a container inlined into this page: a caption over the members
/// listed under it. No chevron, though it names a container — its members are
/// already on screen, which is the whole point of inlining them. Its context
/// menu carries the container's own operations, so inlining stays a
/// presentation default rather than a cage.
private struct GroupHeaderRow<Model: PageDriving>: View {
    let item: Model.Pages.Page.Item
    @ObservedObject var model: Model
    let inset: Int
    let first: Bool

    var body: some View {
        HStack(spacing: 8) {
            Text(groupName(item).uppercased())
                .font(.system(size: 11, weight: .semibold))
                .tracking(0.5)
                .foregroundStyle(.secondary)
            Rectangle()
                .fill(Color.primary.opacity(0.07))
                .frame(height: 1)
        }
        .padding(.horizontal, 14)
        .padding(.leading, CGFloat(inset) * 16)
        .padding(.top, first ? 12 : 16)
        .padding(.bottom, 6)
        .contentShape(Rectangle())
        .contextMenu { PageRowMenu(item: item, model: model) }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(groupName(item))
        .accessibilityAddTraits(.isHeader)
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
    /// The drawn indentation — the layout's rebased rank, not `item.inset`.
    let inset: Int
    let role: PaneRole
    /// The key is a name somebody chose, shown as written (``verbatimKeyIds``).
    let verbatimKey: Bool
    /// Whether this row's card draws icon tiles (``cardShowsTiles``).
    let showsTile: Bool
    @FocusState private var focused: Bool
    @FocusState private var keyFocused: Bool
    /// The pointer is over the row, which is where a click starts an edit.
    @State private var hovering = false

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
                .accessibilityLabel(rowAccessibilityLabel(item, verbatim: verbatimKey))
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
        if let note = item.note { return note }
        if let link = item.linkLabel { return "Opens \(link)" }
        if item.isReadonly { return "Maintained automatically" }
        return ""
    }

    private var core: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 12) {
                if showsTile {
                    IconTile(label: item.label, kind: item.kind, icon: item.icon, tint: item.tint)
                }
                name
                Spacer(minLength: 8)
                trailing
            }
            noticeLine
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .padding(.leading, CGFloat(inset) * 16)
        .frame(minHeight: 44)
        .background(selected ? Color.accentColor.opacity(0.12) : Color.clear)
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .onTapGesture { model.pageActivate(item) }
        .contextMenu { PageRowMenu(item: item, model: model) }
    }

    /// What the last thing done to this row came to, when it was refused or
    /// came with a warning: said here, under the value it is about, rather than
    /// in a bar the eye has to go and find. A line of its own across the row,
    /// under the name — squeezed in beside the name, it would take the room
    /// the field needs exactly when the field is open.
    @ViewBuilder private var noticeLine: some View {
        if let notice = model.notice, notice.shows(under: item.id) {
            let mark = theme.marker(forSeverity: notice.kind == .rejected ? "error" : "warning")
            Label(notice.message, systemImage: mark.symbol)
                .font(.system(size: 12))
                .foregroundStyle(mark.color)
                .lineLimit(3)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.leading, showsTile ? 40 : 0)
                .help(notice.message)
                .accessibilityLabel(notice.message)
        }
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
                // What the host found about this row, when it found anything.
                // Above the note, and in the severity's colour: a finding is
                // about *this document as it stands*, where a description is
                // about the field in general, and the one that can be acted on
                // reads first.
                if let message = item.annotationMessage {
                    let mark = theme.marker(forSeverity: item.annotationSeverity ?? "info")
                    Label(message, systemImage: mark.symbol)
                        .font(.system(size: 11))
                        .foregroundStyle(mark.color)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                // The schema's help text, or failing that the comment the file
                // wrote above the entry. One line: a row is a row, and a
                // paragraph under one of them would turn a list you scan into a
                // page you read. The full text is the tooltip.
                if let note = item.note {
                    Text(note)
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
            }
            .help(item.description ?? item.leadingComment ?? "")
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
            Text(rowName(item, verbatim: verbatimKey))
                .font(.system(size: 15))
                .lineLimit(1)
        }
    }

    /// The file's aside on the value (`port: 8080 # dev`), drawn dimmed ahead
    /// of whatever the row shows for the value itself, the way it reads in
    /// the file — and dropped first when the row is short of room.
    @ViewBuilder private var aside: some View {
        if let comment = item.trailingComment, !comment.isEmpty {
            Text(comment)
                .font(.system(size: 12))
                .foregroundStyle(.tertiary)
                .lineLimit(1)
                .truncationMode(.tail)
                .layoutPriority(-1)
                .accessibilityLabel("Comment: \(comment)")
        }
    }

    @ViewBuilder private var trailing: some View {
        if isDrill {
            HStack(spacing: 6) {
                aside
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
                .accessibilityLabel(rowAccessibilityLabel(item, verbatim: verbatimKey))
        } else if isEditing {
            HStack(spacing: 6) {
                TextField("value", text: $model.editBuffer)
                    .textFieldStyle(.plain)
                    .multilineTextAlignment(.trailing)
                    .font(.system(size: 15, design: valueDesign))
                    .valueField(.editing)
                    .accessibilityLabel(rowAccessibilityLabel(item, verbatim: verbatimKey))
                    .focused($focused)
                    .onSubmit { model.commitEdit() }
                    #if os(macOS)
                    .onExitCommand { model.cancelEdit() }
                    #endif
                    .onAppear { focused = true }
                if numeric {
                    Stepper("", onIncrement: { step(+1) }, onDecrement: { step(-1) })
                        .labelsHidden()
                        .accessibilityLabel("Adjust \(rowAccessibilityLabel(item, verbatim: verbatimKey))")
                }
            }
        } else {
            HStack(spacing: 8) {
                aside
                Text(item.preview.isEmpty ? "Not set" : item.preview)
                    .font(.system(size: 15, design: valueDesign))
                    .foregroundStyle(item.preview.isEmpty
                                     ? Color.gray.opacity(0.8)
                                     : FlowerPalette.value(forKind: item.kind))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .valueField(hovering ? .hovered : .resting)
                    .textCursorOnHover()
            }
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

/// How a typed value's field shows itself.
enum ValueFieldState {
    /// Nothing is pointing at it.
    case resting
    /// A pointer is over its row, so a click would start typing.
    case hovered
    /// It is being typed in.
    case editing
}

/// The one shape a text value has, at rest and while typed in, so that
/// starting an edit lights the field up where the value already is instead of
/// swapping in a box of some other size.
///
/// The field says it is editable the way the platform says so. A Mac shows it
/// when the pointer arrives, as System Settings does, because a page of
/// permanent boxes reads as a web form. Touch has no pointer to arrive, and a
/// tap already starts the edit, so iOS keeps it showing — faintly — and lets an
/// iPad pointer brighten it with the system hover effect.
private struct ValueField: ViewModifier {
    let state: ValueFieldState

    private var fill: Double {
        switch state {
        case .editing: return 0.07
        case .hovered: return 0.06
        case .resting:
            #if os(iOS)
            return 0.04
            #else
            return 0
            #endif
        }
    }

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: 6, style: .continuous)
        content
            .padding(.horizontal, 7)
            .padding(.vertical, 3)
            .background(shape.fill(Color.primary.opacity(fill)))
            .overlay(shape.strokeBorder(Color.accentColor.opacity(state == .editing ? 0.7 : 0),
                                        lineWidth: 1.5))
            // The padding is the field's, not the row's: the text sits where
            // the row's other values do, and the field grows out around it.
            .padding(.horizontal, -7)
            #if os(iOS)
            .hoverEffect(.highlight)
            #endif
            .animation(.easeOut(duration: 0.12), value: state)
    }
}

/// The text cursor over a value a click would start typing in — the pointer's
/// half of saying "editable". Pushed and popped in pairs, and popped on the way
/// out of the hierarchy too: a click swaps the value for its editor while the
/// pointer is still inside, and the `onHover(false)` that would have balanced
/// the push never comes.
private struct TextCursorOnHover: ViewModifier {
    @State private var pushed = false

    func body(content: Content) -> some View {
        #if os(macOS)
        content
            .onHover { inside in
                if inside, !pushed { NSCursor.iBeam.push(); pushed = true }
                else if !inside, pushed { NSCursor.pop(); pushed = false }
            }
            .onDisappear {
                if pushed { NSCursor.pop(); pushed = false }
            }
        #else
        content
        #endif
    }
}

extension View {
    func valueField(_ state: ValueFieldState) -> some View {
        modifier(ValueField(state: state))
    }

    func textCursorOnHover() -> some View {
        modifier(TextCursorOnHover())
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
                .textFieldStyle(.plain)
                .multilineTextAlignment(.trailing)
                .font(.system(size: 15))
                .valueField(.editing)
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
    /// Whether the row can move that way, judged on whichever pane lists it —
    /// the page being edited, or the one it was opened from.
    private func movable(by offset: Int) -> Bool {
        let pages = model.pages
        if pages.page.items.contains(where: { $0.id == item.id }) {
            return canMove(item.id, by: offset, in: pages.page)
        }
        if let parent = pages.parent, parent.items.contains(where: { $0.id == item.id }) {
            return canMove(item.id, by: offset, in: parent)
        }
        return true
    }

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
                .disabled(!movable(by: -1))
            Button("Move Down") { model.moveItemDown(item) }
                .disabled(!movable(by: 1))
            Divider()
            Button("Delete", role: .destructive) { model.delete(item) }
        }
    }
}

// ── A scalar sequence as chips ────────────────────────────────────────────────

/// A scalar sequence as one settings row: the name on the left, the members as
/// removable chips on the right, with the add control among them.
///
/// The alternative spends a row per member on labels that are pure position —
/// `[0]`, `[1]` — for values that are each a word or two. A tag list is one
/// fact about the document, and this draws it as one.
///
/// Every chip is still its own node: tapping one edits it in place through the
/// same buffer every editor shares, deleting one deletes that member, and the
/// header's context menu keeps the sequence's own operations.
private struct PageChipsRow<Model: PageDriving>: View {
    typealias Item = Model.Pages.Page.Item

    let header: Item
    let members: [(index: Int, item: Item)]
    @ObservedObject var model: Model
    let selected: Bool
    let inset: Int
    let role: PaneRole
    let showsTile: Bool
    @FocusState private var chipFocused: Bool

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            if showsTile {
                IconTile(label: header.label, kind: header.kind,
                         icon: header.icon, tint: header.tint)
            }
            Text(rowName(header))
                .font(.system(size: 15))
                .lineLimit(1)
                .padding(.top, 5)
            Spacer(minLength: 8)
            FlowWrap(spacing: 6) {
                ForEach(members, id: \.item.id) { member in
                    chip(for: member.item)
                }
                if role == .cursor, !header.isReadonly {
                    Button { model.pageAddChild(id: header.id) } label: {
                        Label("Add", systemImage: "plus").labelStyle(.titleOnly)
                            .font(.system(size: 13, weight: .medium))
                            .padding(.horizontal, 11).padding(.vertical, 4)
                            .overlay(Capsule().strokeBorder(Color.secondary.opacity(0.4),
                                                            style: StrokeStyle(lineWidth: 1, dash: [3])))
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Add to \(rowAccessibilityLabel(header))")
                }
            }
            .frame(maxWidth: 340, alignment: .trailing)
            .padding(.vertical, 5)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 4)
        .padding(.leading, CGFloat(inset) * 16)
        .frame(minHeight: 44)
        .background(selected ? Color.accentColor.opacity(0.12) : Color.clear)
        .contentShape(Rectangle())
        .contextMenu { PageRowMenu(item: header, model: model) }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(rowAccessibilityLabel(header))
        .accessibilityIdentifier(header.id)
    }

    @ViewBuilder private func chip(for item: Item) -> some View {
        if model.editingId == item.id {
            TextField("", text: $model.editBuffer)
                .textFieldStyle(.plain)
                .font(.system(size: 13, weight: .medium))
                .frame(width: 70)
                .padding(.horizontal, 10).padding(.vertical, 4)
                .background(Capsule().fill(Color.accentColor.opacity(0.12)))
                .accessibilityLabel("Edit \(rowAccessibilityLabel(item))")
                .focused($chipFocused)
                .onSubmit { model.commitEdit() }
                #if os(macOS)
                .onExitCommand { model.cancelEdit() }
                #endif
                .onAppear { chipFocused = true }
        } else {
            HStack(spacing: 5) {
                Button { model.beginEdit(item) } label: {
                    Text(item.preview.isEmpty ? "—" : item.preview)
                        .font(.system(size: 13, weight: .medium))
                }
                .buttonStyle(.plain)
                .disabled(header.isReadonly)
                .accessibilityLabel(item.preview.isEmpty ? "Not set" : item.preview)
                if !header.isReadonly {
                    Button { model.delete(item) } label: {
                        Image(systemName: "xmark").font(.system(size: 9, weight: .bold))
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.secondary)
                    .accessibilityLabel("Delete \(item.preview)")
                }
            }
            .padding(.leading, 11)
            .padding(.trailing, header.isReadonly ? 11 : 7)
            .padding(.vertical, 4)
            .background(Capsule().fill(Color.accentColor.opacity(0.14)))
            .foregroundStyle(Color.accentColor)
        }
    }
}

/// The platform's "no": the alert sound on a Mac, the error haptic on a phone.
func signalRefusal() {
    #if os(macOS)
    NSSound.beep()
    #elseif os(iOS)
    UINotificationFeedbackGenerator().notificationOccurred(.error)
    #endif
}

// ── A minimal wrapping HStack for chips ───────────────────────────────────────

/// Lays children left→right, wrapping to new lines — for tag chips. A small
/// self-contained flow layout (SwiftUI's `Layout`, available on the package's
/// deployment targets).
struct FlowWrap: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxWidth = proposal.width ?? .infinity
        var rows: [[CGSize]] = [[]]
        var x: CGFloat = 0
        for v in subviews {
            let s = v.sizeThatFits(.unspecified)
            if x + s.width > maxWidth, !rows[rows.count - 1].isEmpty {
                rows.append([]); x = 0
            }
            rows[rows.count - 1].append(s); x += s.width + spacing
        }
        let height = rows.reduce(CGFloat(0)) { acc, row in
            acc + (row.map(\.height).max() ?? 0) + spacing
        } - (rows.isEmpty ? 0 : spacing)
        let width = rows.map { $0.reduce(CGFloat(0)) { $0 + $1.width + spacing } - spacing }.max() ?? 0
        return CGSize(width: min(width, maxWidth), height: max(height, 0))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let maxWidth = bounds.width
        var x = bounds.minX
        var y = bounds.minY
        var lineHeight: CGFloat = 0
        for v in subviews {
            let s = v.sizeThatFits(.unspecified)
            if x + s.width > bounds.minX + maxWidth, x > bounds.minX {
                x = bounds.minX; y += lineHeight + spacing; lineHeight = 0
            }
            v.place(at: CGPoint(x: x, y: y), anchor: .topLeading, proposal: ProposedViewSize(s))
            x += s.width + spacing
            lineHeight = max(lineHeight, s.height)
        }
    }
}
