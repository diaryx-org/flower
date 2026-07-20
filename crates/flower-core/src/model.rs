//! The frontend-neutral editor model and its structural operations.
//!
//! The `fig::Editor` is the single source of truth: it owns the document's
//! source bytes and applies every edit as a lossless, path-addressed splice.
//! After each mutation we re-derive the `Value` tree (and the flat rows) from
//! `Editor::source()`, so the model always reflects the canonical bytes.
//!
//! This type holds no filesystem or terminal state. The embedder constructs it
//! from bytes ([`Model::new`]), renders [`Model::rows`], drives the navigation
//! and edit methods, and persists [`Model::source_snapshot`] however it likes.

use std::collections::HashSet;

use anyhow::Result;
use fig::{Document, Editor, Format, Value};

use crate::tree::{self, Row, Seg};

/// Interaction mode: normal navigation, or editing a scalar's text.
pub enum Mode {
    Normal,
    Editing { buffer: String },
}

pub struct Model {
    format: Format,
    editor: Editor,

    /// Derived view state, rebuilt from `editor.source()` after every edit.
    value: Value,
    pub rows: Vec<Row>,
    collapsed: HashSet<Vec<Seg>>,

    pub selected: usize,
    pub mode: Mode,
    pub status: String,
    pub dirty: bool,
}

impl Model {
    /// Build a model over a copy of `source` parsed as `format`.
    pub fn new(source: &[u8], format: Format) -> Result<Self> {
        let editor = Editor::open(source, format)
            .map_err(|e| anyhow::anyhow!("fig failed to parse the document: {e}"))?;

        let mut model = Model {
            format,
            editor,
            value: Value::Null,
            rows: Vec::new(),
            collapsed: HashSet::new(),
            selected: 0,
            mode: Mode::Normal,
            status: "opened".to_string(),
            dirty: false,
        };
        model.reload()?;
        Ok(model)
    }

    pub fn format(&self) -> Format {
        self.format
    }

    /// A copy of the editor's current (canonical) source — what the embedder
    /// writes to disk on save.
    pub fn source_snapshot(&self) -> String {
        self.editor
            .source()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    /// Clear the dirty flag after the embedder has persisted the source.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    // ── view derivation ───────────────────────────────────────────────────────

    /// Re-derive `value` + `rows` from the editor's current source.
    fn reload(&mut self) -> Result<()> {
        let source = self
            .editor
            .source()
            .map_err(|e| anyhow::anyhow!("reading edited source: {e}"))?
            .to_string();
        let doc = Document::parse(source.as_bytes(), self.format)
            .map_err(|e| anyhow::anyhow!("reparsing edited source: {e}"))?;
        self.value = doc
            .to_value()
            .map_err(|e| anyhow::anyhow!("building value tree: {e}"))?;
        self.rebuild_rows();
        Ok(())
    }

    fn rebuild_rows(&mut self) {
        self.rows = tree::build_rows(&self.value, &self.collapsed);
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
    }

    fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    /// Re-anchor selection onto `path` after a rebuild, or clamp if it's gone.
    fn select_path(&mut self, path: &[Seg]) {
        if let Some(i) = self.rows.iter().position(|r| r.path == path) {
            self.selected = i;
        } else if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
    }

    // ── navigation ────────────────────────────────────────────────────────────

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.rows.len() {
            self.selected += 1;
        }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// `l`: expand a collapsed container, else step into its first child.
    pub fn expand_or_enter(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if row.is_container() {
            if !row.expanded {
                let path = row.path.clone();
                self.collapsed.remove(&path);
                self.rebuild_rows();
                self.select_path(&path);
            } else if self.selected + 1 < self.rows.len()
                && self.rows[self.selected + 1].depth > row.depth
            {
                self.selected += 1;
            }
        }
    }

    /// `h`: collapse an expanded container, else step out to the parent row.
    pub fn collapse_or_leave(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if row.is_container() && row.expanded {
            let path = row.path.clone();
            self.collapsed.insert(path.clone());
            self.rebuild_rows();
            self.select_path(&path);
            return;
        }
        // Step out: the nearest earlier row at a shallower depth is the parent.
        let depth = row.depth;
        if depth == 0 {
            return;
        }
        for i in (0..self.selected).rev() {
            if self.rows[i].depth < depth {
                self.selected = i;
                return;
            }
        }
    }

    /// `Enter`/`Space`: toggle a container's expansion, or edit a scalar.
    pub fn activate(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if row.is_container() {
            let path = row.path.clone();
            if row.expanded {
                self.collapsed.insert(path.clone());
            } else {
                self.collapsed.remove(&path);
            }
            self.rebuild_rows();
            self.select_path(&path);
        } else {
            self.begin_edit();
        }
    }

    // ── editing ───────────────────────────────────────────────────────────────

    pub fn begin_edit(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if !row.is_scalar() {
            self.status = "can only edit scalar values".to_string();
            return;
        }
        let seed = self
            .value_at(&row.path)
            .map(|v| tree::edit_seed(&v))
            .unwrap_or_default();
        self.mode = Mode::Editing { buffer: seed };
    }

    pub fn edit_push(&mut self, c: char) {
        if let Mode::Editing { buffer } = &mut self.mode {
            buffer.push(c);
        }
    }

    pub fn edit_backspace(&mut self) {
        if let Mode::Editing { buffer } = &mut self.mode {
            buffer.pop();
        }
    }

    pub fn edit_cancel(&mut self) {
        self.mode = Mode::Normal;
        self.status = "edit cancelled".to_string();
    }

    pub fn edit_commit(&mut self) {
        let Mode::Editing { buffer } = &mut self.mode else {
            return;
        };
        let buffer = std::mem::take(buffer);
        self.mode = Mode::Normal;

        let Some(row) = self.selected_row() else {
            return;
        };
        let path = row.path.clone();
        let value = tree::parse_scalar(&buffer);

        match self.editor.replace_value(&tree::to_fig(&path), value) {
            Ok(()) => self.after_edit(&path, "value updated"),
            // fig rolled the splice back; the document is untouched.
            Err(e) => self.status = format!("rejected: {e}"),
        }
    }

    /// `x`: delete the selected mapping entry or sequence item.
    pub fn delete_selected(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let path = row.path.clone();
        let result = match path.last() {
            Some(Seg::Index(i)) => {
                let parent = tree::to_fig(&path[..path.len() - 1]);
                self.editor.remove_item(&parent, *i)
            }
            Some(Seg::Key(_)) => self.editor.delete(&tree::to_fig(&path)),
            None => {
                self.status = "cannot delete the document root".to_string();
                return;
            }
        };
        match result {
            Ok(()) => {
                let parent = path[..path.len() - 1].to_vec();
                self.after_edit(&parent, "deleted");
            }
            Err(e) => self.status = format!("delete failed: {e}"),
        }
    }

    /// Shared tail of a successful mutation: refresh the view, re-anchor
    /// selection, mark dirty, set the status line.
    fn after_edit(&mut self, anchor: &[Seg], msg: &str) {
        if let Err(e) = self.reload() {
            self.status = format!("view refresh failed: {e}");
            return;
        }
        self.select_path(anchor);
        self.dirty = true;
        self.status = msg.to_string();
    }

    /// Resolve the live `Value` at a path (for seeding the edit buffer).
    fn value_at(&self, path: &[Seg]) -> Option<Value> {
        let mut cur = &self.value;
        for seg in path {
            cur = match (seg, cur) {
                (Seg::Key(k), Value::Map(entries)) => {
                    &entries
                        .iter()
                        .find(|(mk, _)| matches!(mk, Value::Str(s) if s == k))?
                        .1
                }
                (Seg::Index(i), Value::Seq(items)) => items.get(*i)?,
                _ => return None,
            };
        }
        Some(cur.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# flower sample config — comments and formatting below should survive edits
title = \"flower\"
version = 1
enabled = true

# the server block
[server]
host = \"localhost\"
port = 8080
tags = [\"alpha\", \"beta\"]

[server.limits]
max_connections = 100
timeout = 30.5
";

    fn sample_model() -> Model {
        Model::new(SAMPLE.as_bytes(), Format::Toml).expect("open sample")
    }

    fn select(model: &mut Model, path: &[Seg]) {
        model.selected = model
            .rows
            .iter()
            .position(|r| r.path == path)
            .unwrap_or_else(|| panic!("no row for {path:?}"));
    }

    fn type_value(model: &mut Model, text: &str) {
        if let Mode::Editing { buffer } = &mut model.mode {
            buffer.clear();
        }
        for c in text.chars() {
            model.edit_push(c);
        }
        model.edit_commit();
    }

    #[test]
    fn edits_a_scalar_losslessly() {
        let mut model = sample_model();

        select(&mut model, &[Seg::Key("version".into())]);
        model.begin_edit();
        type_value(&mut model, "2");

        let src = model.source_snapshot();
        assert!(src.contains("version = 2"), "value changed:\n{src}");
        assert!(src.contains("# the server block"), "comment preserved:\n{src}");
        assert!(
            src.contains("# flower sample config"),
            "header preserved:\n{src}"
        );
        assert!(model.dirty);
    }

    #[test]
    fn edits_a_nested_string() {
        let mut model = sample_model();

        select(
            &mut model,
            &[Seg::Key("server".into()), Seg::Key("host".into())],
        );
        model.begin_edit();
        type_value(&mut model, "example.com");

        let src = model.source_snapshot();
        assert!(src.contains("host = \"example.com\""), "nested edit:\n{src}");
        assert!(src.contains("port = 8080"), "sibling untouched:\n{src}");
    }

    #[test]
    fn deletes_a_key() {
        let mut model = sample_model();

        select(&mut model, &[Seg::Key("enabled".into())]);
        model.delete_selected();

        let src = model.source_snapshot();
        assert!(!src.contains("enabled = true"), "key removed:\n{src}");
        assert!(src.contains("title = \"flower\""), "siblings kept:\n{src}");
    }

    #[test]
    fn navigation_folds_and_reanchors() {
        let mut model = sample_model();

        select(&mut model, &[Seg::Key("server".into())]);
        model.collapse_or_leave();
        assert!(
            !model
                .rows
                .iter()
                .any(|r| r.path == [Seg::Key("server".into()), Seg::Key("host".into())]),
            "collapsed children hidden"
        );
        assert_eq!(model.rows[model.selected].path, [Seg::Key("server".into())]);
    }
}
