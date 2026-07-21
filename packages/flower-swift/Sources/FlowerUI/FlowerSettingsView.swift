//  FlowerSettingsView.swift
//
//  The settings-panel face of the editor. Where the tree view rendered the flat
//  row list one indented line at a time, this reads those rows as structure and
//  presents each value as a native control — the config file as a System-Settings
//  screen. Everything here is inferred from the *shape* of the value (its kind and
//  key name); nothing needs a schema. A later schema/comment layer refines the
//  icons, section titles, enums, and descriptions.
//
//  The model still drives by row `id`/index, so selection and every edit stay
//  exactly as correct as in the tree view — this is a rendering change only.

import SwiftUI
import FlowerFFI

// ── Structure: rebuild a tree from the flat, depth-tagged row list ─────────────

/// One node of the reconstructed document tree: a row plus its (visible) children.
/// Built from `DocView.rows`, which is pre-order with a `depth` per row — so a
/// simple recursive parse recovers the nesting. Collapsed containers contribute
/// no child rows, so their `children` is empty (rendered as a count).
struct FlowerNode: Identifiable {
    let row: RowView
    let children: [FlowerNode]
    var id: String { row.id }

    /// A sequence whose (visible) items are all scalars — rendered inline as chips
    /// rather than as its own rows.
    var isScalarList: Bool {
        row.kind == "seq" && !children.isEmpty && children.allSatisfy { !$0.row.isContainer }
    }
}

/// Recover the node tree from the flat pre-order row list.
func buildNodes(_ rows: [RowView]) -> [FlowerNode] {
    var i = 0
    return parseLevel(rows, &i, depth: 0)
}

private func parseLevel(_ rows: [RowView], _ i: inout Int, depth: Int) -> [FlowerNode] {
    var out: [FlowerNode] = []
    while i < rows.count, Int(rows[i].depth) == depth {
        let row = rows[i]
        i += 1
        var children: [FlowerNode] = []
        if i < rows.count, Int(rows[i].depth) > depth {
            children = parseLevel(rows, &i, depth: depth + 1)
        }
        out.append(FlowerNode(row: row, children: children))
    }
    return out
}

/// A grouped section of the settings screen: a run of top-level rows under an
/// optional header. Consecutive non-mapping rows collect into an untitled
/// "general" card; each top-level mapping becomes its own titled section.
struct SettingsSection: Identifiable {
    let id: String
    let title: String?
    let container: FlowerNode?   // the mapping node backing a titled section, if any
    let nodes: [FlowerNode]
}

func makeSections(_ roots: [FlowerNode]) -> [SettingsSection] {
    var result: [SettingsSection] = []
    var general: [FlowerNode] = []
    func flush() {
        if !general.isEmpty {
            result.append(SettingsSection(id: "general-\(result.count)", title: nil,
                                          container: nil, nodes: general))
            general = []
        }
    }
    for node in roots {
        if node.row.kind == "map" {
            flush()
            result.append(SettingsSection(id: node.id, title: prettify(node.row.label),
                                          container: node, nodes: node.children))
        } else {
            general.append(node)
        }
    }
    flush()
    return result
}

/// Turn a raw config key into a settings-style display name: `max_connections`
/// → "Max Connections". Presentation only — renames still target the raw key.
func prettify(_ key: String) -> String {
    key.split(whereSeparator: { $0 == "_" || $0 == "-" || $0 == "." })
        .map { $0.prefix(1).uppercased() + String($0.dropFirst()) }
        .joined(separator: " ")
}

// ── The screen ────────────────────────────────────────────────────────────────

/// The settings-panel editor surface. Renders the document as grouped cards of
/// type-aware rows.
public struct FlowerEditor: View {
    @ObservedObject private var model: FlowerModel
    private let theme: FlowerTheme

    public init(model: FlowerModel, theme: FlowerTheme = .default) {
        self.model = model
        self.theme = theme
    }

    private var sections: [SettingsSection] { makeSections(buildNodes(model.state.rows)) }

    public var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 22) {
                ForEach(sections) { section in
                    SectionView(section: section, model: model, theme: theme)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 18)
            .frame(maxWidth: 640, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
    }
}

private struct SectionView: View {
    let section: SettingsSection
    @ObservedObject var model: FlowerModel
    let theme: FlowerTheme

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            if let title = section.title {
                Text(title.uppercased())
                    .font(.system(size: 12, weight: .semibold))
                    .tracking(0.6)
                    .foregroundStyle(.tertiary)
                    .padding(.leading, 14)
            }
            VStack(spacing: 0) {
                if section.nodes.isEmpty, let container = section.container {
                    // A collapsed top-level mapping: offer to reveal its fields.
                    Button { model.toggle(container.row) } label: {
                        HStack {
                            Text("Show \(container.row.preview.trimmingCharacters(in: CharacterSet(charactersIn: "{}"))) fields")
                                .foregroundStyle(.secondary)
                            Spacer()
                            Image(systemName: "chevron.right").font(.caption).foregroundStyle(.tertiary)
                        }
                        .padding(.horizontal, 14).padding(.vertical, 12)
                    }
                    .buttonStyle(.plain)
                } else {
                    ForEach(Array(section.nodes.enumerated()), id: \.element.id) { idx, node in
                        if idx > 0 { rowDivider }
                        NodeView(node: node, model: model, theme: theme, indent: 0)
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
    }

    private var rowDivider: some View {
        Divider().padding(.leading, 57)
    }

    private var cardBackground: some View {
        #if canImport(UIKit)
        Color(.secondarySystemGroupedBackground)
        #else
        Color(nsColor: .controlBackgroundColor)
        #endif
    }
}

/// One node, dispatched to the right presentation: a container becomes a
/// disclosure (or inline chips for a scalar list); a scalar becomes a setting row.
private struct NodeView: View {
    let node: FlowerNode
    @ObservedObject var model: FlowerModel
    let theme: FlowerTheme
    let indent: Int

    var body: some View {
        if node.isScalarList {
            ChipsRow(node: node, model: model, theme: theme, indent: indent)
        } else if node.row.isContainer {
            DisclosureRow(node: node, model: model, theme: theme, indent: indent)
        } else {
            SettingRow(row: node.row, model: model, theme: theme, indent: indent)
        }
    }
}

// ── Rows ───────────────────────────────────────────────────────────────────────

/// A scalar setting: icon tile, label, and a type-aware control on the right.
private struct SettingRow: View {
    let row: RowView
    @ObservedObject var model: FlowerModel
    let theme: FlowerTheme
    let indent: Int
    @FocusState private var focused: Bool
    @FocusState private var keyFocused: Bool

    private var isSelected: Bool { model.selectedRow?.id == row.id }
    private var isEditing: Bool { model.editingId == row.id }
    private var isRenaming: Bool { model.renamingId == row.id }
    private var isItem: Bool { !row.canRename }

    var body: some View {
        HStack(spacing: 12) {
            IconTile(label: row.label, kind: row.kind)
            label
            Spacer(minLength: 8)
            control
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .padding(.leading, CGFloat(indent) * 16)
        .frame(minHeight: 44)
        .background(isSelected ? Color.accentColor.opacity(0.12) : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture { model.select(row) }
        .contextMenu { RowMenu(node: FlowerNode(row: row, children: []), model: model) }
    }

    @ViewBuilder private var label: some View {
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
        } else {
            Text(isItem ? row.label : prettify(row.label))
                .font(.system(size: 15, weight: .regular))
        }
    }

    @ViewBuilder private var control: some View {
        switch row.kind {
        case "bool":
            Toggle("", isOn: Binding(get: { row.preview == "true" },
                                     set: { model.setBool(row, $0) }))
                .labelsHidden()
                .toggleStyle(.switch)
        default:
            if isEditing {
                FieldEditor(model: model, numeric: row.kind == "int" || row.kind == "float",
                            focused: $focused)
            } else {
                Button { model.beginEdit(row) } label: {
                    Text(row.preview.isEmpty ? "Not set" : row.preview)
                        .font(.system(size: 15, design: valueDesign))
                        .foregroundStyle(row.preview.isEmpty ? Color.gray.opacity(0.8) : valueColor)
                        .lineLimit(1)
                }
                .buttonStyle(.plain)
            }
        }
    }

    private var valueDesign: Font.Design {
        (row.kind == "str" || row.kind == "null") ? .default : .monospaced
    }
    private var valueColor: Color { FlowerPalette.value(forKind: row.kind) }
}

/// The shared value text field, with numeric ± steppers when editing a number.
private struct FieldEditor: View {
    @ObservedObject var model: FlowerModel
    let numeric: Bool
    var focused: FocusState<Bool>.Binding

    var body: some View {
        HStack(spacing: 6) {
            TextField("value", text: $model.editBuffer)
                .textFieldStyle(.roundedBorder)
                .font(.system(size: 14, design: numeric ? .monospaced : .default))
                .frame(maxWidth: 150)
                .focused(focused)
                .onSubmit { model.commitEdit() }
                #if os(macOS)
                .onExitCommand { model.cancelEdit() }
                #endif
                .onAppear { focused.wrappedValue = true }
            if numeric {
                Stepper("", onIncrement: { step(+1) }, onDecrement: { step(-1) })
                    .labelsHidden()
            }
        }
    }

    private func step(_ delta: Int) {
        let t = model.editBuffer.trimmingCharacters(in: .whitespaces)
        if let i = Int(t) { model.editBuffer = String(i + delta) }
        else if let d = Double(t) { model.editBuffer = String(d + Double(delta)) }
    }
}

/// A nested container: a header row (icon, name, count, chevron) that toggles it,
/// with its children rendered indented below when expanded.
private struct DisclosureRow: View {
    let node: FlowerNode
    @ObservedObject var model: FlowerModel
    let theme: FlowerTheme
    let indent: Int

    private var row: RowView { node.row }
    private var isSelected: Bool { model.selectedRow?.id == row.id }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                IconTile(label: row.label, kind: row.kind)
                Text(row.canRename ? prettify(row.label) : row.label)
                    .font(.system(size: 15, weight: .medium))
                Spacer(minLength: 8)
                Text(countText)
                    .font(.system(size: 14))
                    .foregroundStyle(.tertiary)
                Image(systemName: row.expanded ? "chevron.down" : "chevron.right")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 9)
            .padding(.leading, CGFloat(indent) * 16)
            .frame(minHeight: 44)
            .background(isSelected ? Color.accentColor.opacity(0.12) : Color.clear)
            .contentShape(Rectangle())
            .onTapGesture { model.toggle(row) }
            .contextMenu { RowMenu(node: node, model: model) }

            if row.expanded {
                ForEach(Array(node.children.enumerated()), id: \.element.id) { idx, child in
                    Divider().padding(.leading, 57 + CGFloat(indent) * 16)
                    NodeView(node: child, model: model, theme: theme, indent: indent + 1)
                }
            }
        }
    }

    private var countText: String {
        let n = row.preview.trimmingCharacters(in: CharacterSet(charactersIn: "{}[]"))
        return row.kind == "seq" ? "\(n) items" : "\(n) fields"
    }
}

/// A scalar sequence rendered inline as removable tag chips with an add control.
private struct ChipsRow: View {
    let node: FlowerNode
    @ObservedObject var model: FlowerModel
    let theme: FlowerTheme
    let indent: Int
    @FocusState private var chipFocused: Bool

    private var row: RowView { node.row }

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            IconTile(label: row.label, kind: row.kind)
            Text(row.canRename ? prettify(row.label) : row.label)
                .font(.system(size: 15, weight: .regular))
                .padding(.top, 3)
            Spacer(minLength: 8)
            FlowWrap(spacing: 6) {
                ForEach(node.children) { child in
                    chip(for: child.row)
                }
                Button { model.addChild(row) } label: {
                    Label("Add", systemImage: "plus").labelStyle(.titleOnly)
                        .font(.system(size: 13, weight: .medium))
                        .padding(.horizontal, 11).padding(.vertical, 4)
                        .overlay(Capsule().strokeBorder(Color.secondary.opacity(0.4),
                                                        style: StrokeStyle(lineWidth: 1, dash: [3])))
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
            .frame(maxWidth: 340, alignment: .trailing)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .padding(.leading, CGFloat(indent) * 16)
        .frame(minHeight: 44)
        .contentShape(Rectangle())
        .contextMenu { RowMenu(node: node, model: model) }
    }

    @ViewBuilder private func chip(for item: RowView) -> some View {
        if model.editingId == item.id {
            TextField("", text: $model.editBuffer)
                .textFieldStyle(.plain)
                .font(.system(size: 13, weight: .medium))
                .frame(width: 70)
                .padding(.horizontal, 10).padding(.vertical, 4)
                .background(Capsule().fill(Color.accentColor.opacity(0.12)))
                .focused($chipFocused)
                .onSubmit { model.commitEdit() }
                .onAppear { chipFocused = true }
        } else {
            HStack(spacing: 5) {
                Button { model.beginEdit(item) } label: {
                    Text(item.preview.isEmpty ? "—" : item.preview)
                        .font(.system(size: 13, weight: .medium))
                }
                .buttonStyle(.plain)
                Button { model.delete(item) } label: {
                    Image(systemName: "xmark").font(.system(size: 9, weight: .bold))
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
            }
            .padding(.leading, 11).padding(.trailing, 7).padding(.vertical, 4)
            .background(Capsule().fill(Color.accentColor.opacity(0.14)))
            .foregroundStyle(Color.accentColor)
        }
    }
}

// ── The shared context menu ─────────────────────────────────────────────────────

private struct RowMenu: View {
    let node: FlowerNode
    @ObservedObject var model: FlowerModel

    var body: some View {
        let row = node.row
        if !row.isContainer {
            Button("Edit Value") { model.beginEdit(row) }
        }
        if row.canRename {
            Button("Rename") { model.beginRename(row) }
        }
        if model.canAddChild(row) {
            Button(row.kind == "seq" ? "Add Item" : "Add Field") { model.addChild(row) }
        }
        if model.canReorder(row) {
            Divider()
            Button("Move Up") { model.moveRowUp(row) }
            Button("Move Down") { model.moveRowDown(row) }
        }
        Divider()
        Button("Delete", role: .destructive) { model.delete(row) }
    }
}

// ── Icon tile ────────────────────────────────────────────────────────────────

/// The colored rounded-square glyph at the head of a row — the device that makes
/// a form read as "settings". Inferred from the key name, falling back to the
/// value kind. A schema layer would override these per key.
struct IconTile: View {
    let label: String
    let kind: String

    var body: some View {
        let spec = FlowerPalette.icon(label: label, kind: kind)
        Image(systemName: spec.symbol)
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(spec.color)
            .frame(width: 28, height: 28)
            .background(RoundedRectangle(cornerRadius: 7).fill(spec.color.opacity(0.16)))
    }
}

/// The inferred-presentation palette: icon + colour per key/kind, and value
/// colours. Pure data — the SwiftUI peer of the mockup's token table.
enum FlowerPalette {
    struct IconSpec { let symbol: String; let color: Color }

    static func icon(label: String, kind: String) -> IconSpec {
        let l = label.lowercased()
        func has(_ needles: [String]) -> Bool { needles.contains { l.contains($0) } }

        // Name-based (most specific first).
        if has(["author", "owner", "user", "creator", "by"]) { return .init(symbol: "person.crop.circle.fill", color: .indigo) }
        if has(["tag", "keyword", "label", "categor"]) { return .init(symbol: "tag.fill", color: .teal) }
        if has(["visib", "privacy", "access", "share", "audience"]) { return .init(symbol: "eye.fill", color: .orange) }
        if has(["pin"]) { return .init(symbol: "pin.fill", color: .pink) }
        if has(["priorit", "rank", "order", "weight", "importan"]) { return .init(symbol: "chart.bar.fill", color: .purple) }
        if has(["tls", "ssl", "secure", "encrypt", "cert", "https", "auth", "token", "secret", "password"]) { return .init(symbol: "lock.shield.fill", color: .green) }
        if has(["host", "url", "domain", "address", "endpoint"]) { return .init(symbol: "globe", color: .blue) }
        if has(["server"]) { return .init(symbol: "server.rack", color: .blue) }
        if has(["port"]) { return .init(symbol: "number", color: .gray) }
        if has(["limit", "max", "min", "quota", "rate", "threshold"]) { return .init(symbol: "slider.horizontal.3", color: .blue) }
        if has(["timeout", "duration", "interval", "expire", "time", "date"]) { return .init(symbol: "clock.fill", color: .orange) }
        if has(["connection", "network"]) { return .init(symbol: "network", color: .blue) }
        if has(["mail", "email"]) { return .init(symbol: "envelope.fill", color: .blue) }
        if has(["path", "dir", "folder", "file"]) { return .init(symbol: "folder.fill", color: .gray) }
        if has(["color", "theme", "appearance", "style"]) { return .init(symbol: "paintpalette.fill", color: .pink) }
        if has(["version"]) { return .init(symbol: "number.circle.fill", color: .gray) }
        if has(["title", "heading", "label"]) { return .init(symbol: "textformat", color: .indigo) }
        if has(["descript", "summary", "note", "comment", "body"]) { return .init(symbol: "text.alignleft", color: .gray) }
        if has(["lang", "locale"]) { return .init(symbol: "character.bubble", color: .teal) }
        if has(["enable", "active", "status", "state"]) { return .init(symbol: "power", color: .green) }
        if has(["count", "size", "length", "amount", "num"]) { return .init(symbol: "number", color: .gray) }

        // Kind-based fallback.
        switch kind {
        case "map": return .init(symbol: "folder.fill", color: .gray)
        case "seq": return .init(symbol: "list.bullet", color: .teal)
        case "bool": return .init(symbol: "switch.2", color: .green)
        case "int", "float": return .init(symbol: "number", color: .blue)
        case "str": return .init(symbol: "textformat", color: .indigo)
        case "ext": return .init(symbol: "curlybraces", color: .orange)
        default: return .init(symbol: "minus.circle", color: .gray)
        }
    }

    static func value(forKind kind: String) -> Color {
        switch kind {
        case "bool": return .purple
        case "int", "float": return .blue
        case "str": return .green
        case "ext": return .orange
        default: return .gray
        }
    }
}

// ── A minimal wrapping HStack for chips ────────────────────────────────────────

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
