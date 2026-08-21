//  FlowerTheme.swift
//
//  Maps a row's semantic `kind` (the renderer class id flower-ffi ships:
//  "null"/"bool"/"int"/"float"/"str"/"ext"/"map"/"seq") onto the SwiftUI
//  presentation — the value colour and a glyph — plus fonts a host may
//  customise. Pure data, so it type-checks and unit-tests without any Rust
//  runtime.

import SwiftUI

/// The look of the rows: fonts for labels and values, and the palette a value's
/// kind resolves to. Customise by constructing one and passing it to
/// `FlowerPages(model:theme:)`.
public struct FlowerTheme {
    public var labelFont: Font
    public var valueFont: Font
    public var indentWidth: CGFloat
    public var rowSpacing: CGFloat

    public init(
        labelFont: Font = .system(.body, design: .default).weight(.medium),
        valueFont: Font = .system(.body, design: .monospaced),
        indentWidth: CGFloat = 16,
        rowSpacing: CGFloat = 2
    ) {
        self.labelFont = labelFont
        self.valueFont = valueFont
        self.indentWidth = indentWidth
        self.rowSpacing = rowSpacing
    }

    public static let `default` = FlowerTheme()

    /// The colour a scalar value is drawn in, by kind — the SwiftUI peer of the
    /// TUI's `value_style`.
    public func color(forKind kind: String) -> Color {
        switch kind {
        case "null": return .secondary
        case "bool": return .purple
        case "int", "float": return .cyan
        case "str": return .green
        case "ext": return .orange
        default: return .secondary // map / seq previews are chrome
        }
    }

    /// The SF Symbol shown at the head of a row: a disclosure state for
    /// containers, a small dot for scalars.
    public func symbol(isContainer: Bool, expanded: Bool) -> String {
        if isContainer {
            return expanded ? "chevron.down" : "chevron.right"
        }
        return "circlebadge.fill"
    }
}
