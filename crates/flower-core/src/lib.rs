//! flower-core — the frontend-neutral structural editing model for config files.
//!
//! Given a config document's bytes and format, it exposes the document as a
//! navigable projection of fig's `Value` — plus structural navigation and
//! path-addressed edits routed through fig's lossless editor. It knows nothing
//! about terminals, GUIs, or the filesystem — a frontend (flower-ratatui,
//! a future flower-gpui, …) renders the projection and drives the model's
//! methods; the embedder owns file I/O.
//!
//! There are two projections, selected by [`ViewMode`], over one document and one
//! set of edits:
//!
//! - the **[`tree`]** — every visible node at once, indented by depth
//!   ([`Model::rows`]). The document as a document.
//! - the **[`page`]** — one container at a time, pushed and popped
//!   ([`Model::page`]), with containers that fit the [`InlineBudget`] inlined.
//!   The document as a settings menu, and the one that stays legible when it is
//!   deep — or, under a generous budget, the whole document on one page.

pub mod backend;
pub mod format;
pub mod model;
pub mod page;
pub mod schema;
pub mod tree;

pub use backend::{Backend, BackendError, EditOp, FigBackend};
pub use format::detect;
pub use model::{Mode, Model, ViewMode};
pub use page::{InlineBudget, ItemKind, Page, PageItem};
pub use schema::{Constraint, FieldRule, FieldRuleExt, Schema};
pub use tree::{Row, VKind};

// The generic, prov-agnostic pieces (path matching, field type, controlled
// vocabulary, presentation hints) live in fig-schema now; re-exported here so
// existing callers importing them from flower_core keep working.
//
// **This list is the reachability boundary.** An embedder that depends on
// flower and not on fig-schema — which is the arrangement flower's facade
// exists to offer — can name a fig-schema type only if it appears here, so a
// new type upstream is invisible downstream until it is added.
//
// The asymmetry is easy to miss because it does not apply to *methods*:
// `FieldRule` and `Schema` are plain aliases (see `schema`), so a new inherent
// method on either arrives free and needs no edit here. Only names need
// naming. An embedder can therefore end up able to call a method and unable to
// name the type it returns, which is a compile error a long way from its
// cause.
//
// It is also not something a semver check can catch: adding a type upstream is
// additive and passes, while remaining unreachable through this list. Adding a
// fig-schema type is two edits, and this is the second.
pub use fig_schema::{
    Cardinality, Consequence, FieldType, Icon, PathPat, Presentation, Seg, SegPat, Severity, Term,
    Tint, Validation, guards_without_terms,
};
