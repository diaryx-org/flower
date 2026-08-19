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
//!   ([`Model::page`]), with small all-scalar groups inlined. The document as a
//!   settings menu, and the one that stays legible when it is deep.

pub mod backend;
pub mod format;
pub mod model;
pub mod page;
pub mod schema;
pub mod tree;

pub use backend::{Backend, BackendError, EditOp, FigBackend};
pub use format::detect;
pub use model::{Mode, Model, ViewMode};
pub use page::{ItemKind, Page, PageItem};
pub use schema::{Constraint, FieldRule, FieldRuleExt, Schema};
pub use tree::{Row, VKind};

// The generic, prov-agnostic pieces (path matching, field type, controlled
// vocabulary, presentation hints) live in fig-schema now; re-exported here so
// existing callers importing them from flower_core keep working.
pub use fig_schema::{
    Cardinality, FieldType, Icon, PathPat, Presentation, Seg, SegPat, Term, Tint, Validation,
};
