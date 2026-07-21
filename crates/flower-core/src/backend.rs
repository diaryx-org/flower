//! The commit sink: what [`Model`](crate::Model) edits *through*.
//!
//! `Model` never talks to a concrete editor. It builds path-addressed [`EditOp`]s
//! and hands them to a [`Backend`], and reads the current tree back via
//! [`Backend::to_value`]. That indirection is the integration seam:
//!
//! - [`FigBackend`] drives a raw [`fig::Editor`] — a standalone config file.
//! - A future prov backend will drive prov's frontmatter editor, applying the
//!   same ops but *also* maintaining inverse links, fixity, and the journal —
//!   so the GUI gets those invariants for free without `Model` knowing about
//!   them.
//!
//! Every op is atomic: on error the document is left exactly as it was (fig
//! reparses and rolls back; a prov backend declines and stages nothing).

use fig::{Format, Value};

use crate::tree::{self, Seg};

/// One path-addressed edit. The vocabulary grows as `Model` gains operations
/// (insert, reorder, comments, …); today it covers what the editor issues.
#[derive(Debug, Clone)]
pub enum EditOp {
    /// Replace the scalar/subtree at `path` with `value`.
    ReplaceValue { path: Vec<Seg>, value: Value },
    /// Delete the mapping entry at `path`.
    DeleteKey { path: Vec<Seg> },
    /// Remove item `index` from the sequence at `seq_path`.
    RemoveItem { seq_path: Vec<Seg>, index: usize },
    /// Insert `key = value` into the mapping at `map_path`.
    InsertKey {
        map_path: Vec<Seg>,
        key: String,
        value: Value,
    },
    /// Append `value` to the sequence at `seq_path`.
    AppendItem { seq_path: Vec<Seg>, value: Value },
    /// Move the item at `from` to `to` in the sequence at `seq_path`.
    MoveItem {
        seq_path: Vec<Seg>,
        from: usize,
        to: usize,
    },
    /// Reorder the mapping at `map_path` so its entries follow `keys`.
    ReorderKeys {
        map_path: Vec<Seg>,
        keys: Vec<String>,
    },
}

/// A backend failure, carrying the underlying message. An error means the edit
/// did not apply; the document is unchanged.
#[derive(Debug)]
pub struct BackendError(pub String);

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BackendError {}

fn err(e: impl std::fmt::Display) -> BackendError {
    BackendError(e.to_string())
}

/// The editing surface `Model` drives. Implementors own the document's bytes and
/// apply edits losslessly.
pub trait Backend {
    /// Apply one edit. Atomic: on `Err`, the document is unchanged.
    fn apply(&mut self, op: EditOp) -> Result<(), BackendError>;

    /// The current value tree to render (for an embed backend, the metadata
    /// region — *not* the whole host file).
    fn to_value(&self) -> Result<Value, BackendError>;

    /// The canonical serialized form the embedder persists on save (for an embed
    /// backend, the full rendered host file).
    fn source(&self) -> Result<String, BackendError>;
}

/// A [`Backend`] over a standalone config file, backed by [`fig::Editor`].
pub struct FigBackend {
    editor: fig::Editor,
    format: Format,
}

impl FigBackend {
    /// Open an editor over a copy of `source` parsed as `format`.
    pub fn open(source: &[u8], format: Format) -> Result<Self, BackendError> {
        let editor = fig::Editor::open(source, format).map_err(err)?;
        Ok(Self { editor, format })
    }
}

impl Backend for FigBackend {
    fn apply(&mut self, op: EditOp) -> Result<(), BackendError> {
        match op {
            EditOp::ReplaceValue { path, value } => self
                .editor
                .replace_value(&tree::to_fig(&path), value)
                .map_err(err),
            EditOp::DeleteKey { path } => self.editor.delete(&tree::to_fig(&path)).map_err(err),
            EditOp::RemoveItem { seq_path, index } => self
                .editor
                .remove_item(&tree::to_fig(&seq_path), index)
                .map_err(err),
            EditOp::InsertKey {
                map_path,
                key,
                value,
            } => self
                .editor
                .insert_value(&tree::to_fig(&map_path), &key, value)
                .map_err(err),
            EditOp::AppendItem { seq_path, value } => self
                .editor
                .append_value(&tree::to_fig(&seq_path), value)
                .map_err(err),
            EditOp::MoveItem {
                seq_path,
                from,
                to,
            } => self
                .editor
                .move_item(&tree::to_fig(&seq_path), from, to)
                .map_err(err),
            EditOp::ReorderKeys { map_path, keys } => self
                .editor
                .reorder_keys(&tree::to_fig(&map_path), &keys)
                .map_err(err),
        }
    }

    fn to_value(&self) -> Result<Value, BackendError> {
        let src = self.editor.source().map_err(err)?;
        let doc = fig::Document::parse(src.as_bytes(), self.format).map_err(err)?;
        doc.to_value().map_err(err)
    }

    fn source(&self) -> Result<String, BackendError> {
        self.editor.source().map(|s| s.to_string()).map_err(err)
    }
}
