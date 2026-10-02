//  FlowerPresentation.swift
//
//  The presentation vocabulary both surfaces share: how a key reads as a name,
//  and which symbol and colour stand for a field.
//
//  Two layers, in that order: what a schema *said* about a field, and — when
//  nothing said anything — what its key and kind suggest. The inference is the
//  floor rather than the answer, because it has to work on a document nobody
//  has described; wherever a schema has described one, it wins.
//
//  Pure functions over strings, with no document behind them, which is why this
//  lives in the FFI-free target alongside the views that use it. Both the tree
//  editor and the page editor draw the same icon for the same field, and they do
//  it by sharing this rather than by two tables that agree until one is edited.

import SwiftUI

/// Turn a raw config key into a settings-style display name: `max_connections`
/// → "Max Connections", `devDependencies` → "Dev Dependencies". Presentation
/// only — renames still target the raw key.
///
/// A key that is not identifier-shaped — `@types/bun`, `src/**`, a key with
/// spaces in it — comes back exactly as written: it is a name somebody chose,
/// not a field name, and title-casing it would show a key the document does
/// not contain.
public func prettify(_ key: String) -> String {
    guard isIdentifierShaped(key) else { return key }
    return keyWords(key)
        .map { $0.prefix(1).uppercased() + String($0.dropFirst()) }
        .joined(separator: " ")
}

/// Whether a key reads as a field name: letters, digits, and the `_` `-` `.`
/// that separate the words of one.
func isIdentifierShaped(_ key: String) -> Bool {
    !key.isEmpty && key.allSatisfy { $0.isLetter || $0.isNumber || $0 == "_" || $0 == "-" || $0 == "." }
}

/// The words of a key: split at `_`, `-` and `.`, and at every camelCase hump —
/// `noEmit` → `no`, `Emit`; `HTTPServer` → `HTTP`, `Server`. Case is kept.
func keyWords(_ key: String) -> [String] {
    var words: [String] = []
    for part in key.split(whereSeparator: { $0 == "_" || $0 == "-" || $0 == "." }) {
        let chars = Array(part)
        var word = ""
        for (i, c) in chars.enumerated() {
            if i > 0, c.isUppercase {
                let prev = chars[i - 1]
                let nextIsLower = i + 1 < chars.count && chars[i + 1].isLowercase
                // `devD…` starts a word; so does the `S` of `HTTPServer`, the
                // last capital of a run that a lowercase letter follows.
                if prev.isLowercase || prev.isNumber || (prev.isUppercase && nextIsLower) {
                    words.append(word)
                    word = ""
                }
            }
            word.append(c)
        }
        if !word.isEmpty { words.append(word) }
    }
    return words
}

/// The colored rounded-square glyph at the head of a row — the device that makes
/// a form read as "settings".
///
/// The schema's `icon` and `tint` when it named them, and the inference from key
/// name and value kind when it did not. `icon` and `tint` are independent: a
/// schema can say a field is dangerous without saying what it looks like, and
/// the inferred symbol then wears the declared colour.
public struct IconTile: View {
    let label: String
    let kind: String
    let icon: String?
    let tint: String?

    public init(label: String, kind: String, icon: String? = nil, tint: String? = nil) {
        self.label = label
        self.kind = kind
        self.icon = icon
        self.tint = tint
    }

    public var body: some View {
        let spec = FlowerPalette.icon(label: label, kind: kind, icon: icon, tint: tint)
        Image(systemName: spec.symbol)
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(spec.color)
            .frame(width: 28, height: 28)
            .background(RoundedRectangle(cornerRadius: 7).fill(spec.color.opacity(0.16)))
    }
}

/// The inferred-presentation palette: icon + colour per key/kind, and value
/// colours. Pure data — the SwiftUI peer of the mockup's token table.
public enum FlowerPalette {
    public struct IconSpec { let symbol: String; let color: Color }

    /// The symbol and colour a row's tile wears, resolved schema-first.
    ///
    /// `icon` and `tint` are what a schema declared, either or both possibly
    /// absent; `label` and `kind` are what the document itself offers, and are
    /// always there. Each half falls back independently, so a schema that names
    /// only one of them is taken at its word about that one instead of being
    /// ignored for being incomplete.
    ///
    /// An unrecognised `icon` name falls back too, rather than rendering as a
    /// missing glyph: the vocabulary is open (fig-schema's `Icon::Other`), so a
    /// name this host has never heard of is a schema talking to some other
    /// frontend, not a schema making a mistake.
    public static func icon(
        label: String,
        kind: String,
        icon: String? = nil,
        tint: String? = nil
    ) -> IconSpec {
        let inferred = inferredIcon(label: label, kind: kind)
        return IconSpec(
            symbol: icon.flatMap(symbol(forSemanticIcon:)) ?? inferred.symbol,
            color: tint.flatMap(color(forTint:)) ?? inferred.color
        )
    }

    /// fig-schema's `Icon` vocabulary in SF Symbols. `nil` for a name outside
    /// it — including `Icon::Other`, which by construction names a symbol set
    /// this host may not be.
    public static func symbol(forSemanticIcon name: String) -> String? {
        switch name {
        case "link": return "link"
        case "enum": return "list.bullet"
        case "toggle": return "switch.2"
        case "lock": return "lock.fill"
        case "globe": return "globe"
        case "clock": return "clock.fill"
        case "tag": return "tag.fill"
        case "text": return "textformat"
        default: return nil
        }
    }

    /// fig-schema's `Tint` vocabulary as theme-adaptive colours. `nil` for a
    /// name outside it.
    ///
    /// `neutral` is deliberately a colour rather than "leave it alone": a schema
    /// saying a field is unremarkable is saying something, and the row that
    /// carries it should not end up louder than the one marked `danger`.
    public static func color(forTint name: String) -> Color? {
        switch name {
        case "accent": return .accentColor
        case "neutral": return .secondary
        case "positive": return .green
        case "warning": return .orange
        case "danger": return .red
        default: return nil
        }
    }

    /// The floor: what a key's name and a value's kind suggest, for a document
    /// no schema describes.
    public static func inferredIcon(label: String, kind: String) -> IconSpec {
        nameIcon(label: label) ?? kindIcon(kind)
    }

    /// Whether a row's tile says something its neighbours' do not: the schema
    /// named its icon or tint, or its key matched a name. A tile that only
    /// restates the value's kind is the same "Aa" on every row of a card of
    /// strings, which is decoration rather than information.
    public static func isDistinctive(label: String, icon: String? = nil, tint: String? = nil) -> Bool {
        icon.flatMap(symbol(forSemanticIcon:)) != nil
            || tint.flatMap(color(forTint:)) != nil
            || nameIcon(label: label) != nil
    }

    /// What a key's name suggests, or `nil` when it suggests nothing.
    ///
    /// Matched on the key's words, not its letters: a needle is the whole word,
    /// or — three letters or longer — the start of one, so `user` finds
    /// `username` and `categor` finds `category`, but `port` no longer finds
    /// `import` and `by` no longer finds `hobby`.
    static func nameIcon(label: String) -> IconSpec? {
        let words = keyWords(label).map { $0.lowercased() }
        func has(_ needles: [String]) -> Bool {
            needles.contains { n in words.contains { $0 == n || (n.count >= 3 && $0.hasPrefix(n)) } }
        }

        // Name-based (most specific first).
        if has(["author", "owner", "user", "creator", "by"]) { return .init(symbol: "person.crop.circle.fill", color: .indigo) }
        if has(["tag", "keyword", "label", "categor"]) { return .init(symbol: "tag.fill", color: .teal) }
        if has(["visib", "privacy", "access", "share", "audience"]) { return .init(symbol: "eye.fill", color: .orange) }
        if has(["pin", "pinned"]) { return .init(symbol: "pin.fill", color: .pink) }
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
        if has(["title", "heading"]) { return .init(symbol: "textformat", color: .indigo) }
        if has(["descript", "summary", "note", "comment", "body"]) { return .init(symbol: "text.alignleft", color: .gray) }
        if has(["lang", "locale"]) { return .init(symbol: "character.bubble", color: .teal) }
        if has(["enable", "active", "status", "state"]) { return .init(symbol: "power", color: .green) }
        if has(["count", "size", "length", "amount", "num"]) { return .init(symbol: "number", color: .gray) }

        return nil
    }

    /// The floor under the floor: what the value's kind suggests.
    static func kindIcon(_ kind: String) -> IconSpec {
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

    /// A value's colour, by kind. Text is secondary, the way a settings row
    /// draws its value: a card of strings in an accent colour shouts over the
    /// names it is meant to sit beside, and says only "this is text" — which
    /// the text already says. The accents are kept for the kinds that are
    /// rarer and worth telling apart at a glance.
    public static func value(forKind kind: String) -> Color {
        switch kind {
        case "bool": return .purple
        case "int", "float": return .blue
        case "str": return .secondary
        case "ext": return .orange
        default: return .gray
        }
    }
}
