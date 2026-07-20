//! flower-core — the frontend-neutral structural editing model for config files.
//!
//! Given a config document's bytes and format, it exposes a navigable tree of
//! [`Row`]s (a projection of fig's `Value`) plus structural navigation and
//! path-addressed edits routed through fig's lossless editor. It knows nothing
//! about terminals, GUIs, or the filesystem — a frontend (flower-ratatui,
//! a future flower-gpui, …) renders [`Model::rows`] and drives the model's
//! methods; the embedder owns file I/O.

pub mod format;
pub mod model;
pub mod tree;

pub use format::detect;
pub use model::{Mode, Model};
pub use tree::{Row, Seg, VKind};
