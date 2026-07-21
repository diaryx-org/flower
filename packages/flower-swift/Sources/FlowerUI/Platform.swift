//  Platform.swift
//
//  The thin AppKit⇄UIKit shim that lets the rest of FlowerUI stay platform-
//  neutral. Colours are the only place macOS and iOS truly diverge for a
//  SwiftUI tree view; everything above this file is written once.

import SwiftUI

#if canImport(UIKit)
import UIKit
public typealias FlowerColor = UIColor
#elseif canImport(AppKit)
import AppKit
public typealias FlowerColor = NSColor
#endif

/// The default semantic colours, resolved to each toolkit's dynamic system
/// colours so light/dark just works on both platforms.
public enum Palette {
    #if canImport(UIKit)
    static var label: FlowerColor { .label }
    static var secondary: FlowerColor { .secondaryLabel }
    static var tertiary: FlowerColor { .tertiaryLabel }
    static var separator: FlowerColor { .separator }
    static var selection: FlowerColor { UIColor.systemBlue.withAlphaComponent(0.18) }
    #elseif canImport(AppKit)
    static var label: FlowerColor { .labelColor }
    static var secondary: FlowerColor { .secondaryLabelColor }
    static var tertiary: FlowerColor { .tertiaryLabelColor }
    static var separator: FlowerColor { .separatorColor }
    static var selection: FlowerColor { .selectedContentBackgroundColor }
    #endif
}
