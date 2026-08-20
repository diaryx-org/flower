//  FlowerPresentation.swift
//
//  The presentation vocabulary both surfaces share: how a key reads as a name,
//  and which symbol and colour stand for a field.
//
//  Pure inference over the two strings core hands a row — its label and its kind
//  — and no document behind it, which is why it lives in the FFI-free target
//  alongside the views that use it. Both the tree editor and the page editor
//  draw the same icon for the same key, and they do it by sharing this rather
//  than by two tables that agree until one of them is edited.

import SwiftUI

/// Turn a raw config key into a settings-style display name: `max_connections`
/// → "Max Connections". Presentation only — renames still target the raw key.
public func prettify(_ key: String) -> String {
    key.split(whereSeparator: { $0 == "_" || $0 == "-" || $0 == "." })
        .map { $0.prefix(1).uppercased() + String($0.dropFirst()) }
        .joined(separator: " ")
}

/// The colored rounded-square glyph at the head of a row — the device that makes
/// a form read as "settings". Inferred from the key name, falling back to the
/// value kind. A schema layer would override these per key.
public struct IconTile: View {
    let label: String
    let kind: String

    public init(label: String, kind: String) {
        self.label = label
        self.kind = kind
    }

    public var body: some View {
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
public enum FlowerPalette {
    public struct IconSpec { let symbol: String; let color: Color }

    public static func icon(label: String, kind: String) -> IconSpec {
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

    public static func value(forKind kind: String) -> Color {
        switch kind {
        case "bool": return .purple
        case "int", "float": return .blue
        case "str": return .green
        case "ext": return .orange
        default: return .gray
        }
    }
}
