//  PageConformance.swift
//
//  What makes this package's generated records renderable by the FFI-free page
//  editor: nothing but the declarations.
//
//  The protocols in FlowerPagesUI were written from the shape of these records,
//  so every conformance below is empty. That is the point of the exercise rather
//  than a happy accident — a protocol that needed adapter properties would be a
//  protocol describing some other document, and the next host would find it as
//  awkward as this one does.
//
//  A second host writes exactly this file over its own binding's records and
//  gets the same page editor, instead of a copy of it that drifts.

@_exported import FlowerPagesUI
import FlowerFFI

// ── the records ───────────────────────────────────────────────────────────────

// Identified by the dotted fig path, like every other node flower names, so a
// `ForEach` can key on it directly.
extension PageItemView: @retroactive Identifiable {}
extension CrumbView: @retroactive Identifiable {}

// `@retroactive` throughout: both sides are imported here — the records from
// the generated binding, the protocols from FlowerPagesUI — and this module owns
// neither. That is exactly the arrangement a second host is in, so the shape of
// this file is the shape of theirs.
extension PageItemView: @retroactive PageItemDisplaying {}
extension CrumbView: @retroactive CrumbDisplaying {}
extension PageView: @retroactive PageDisplaying {}
extension PagesView: @retroactive PagesDisplaying {}

// ── the model ─────────────────────────────────────────────────────────────────

/// `FlowerModel` already spoke this vocabulary — it was written as the one thing
/// `FlowerPages` drove. Naming it as a protocol conformance changes nothing here
/// and makes the view reusable there.
extension FlowerModel: PageDriving {}
