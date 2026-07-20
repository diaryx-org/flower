//! Editor state and the structural operations.
//!
//! The `fig::Editor` is the single source of truth: it owns the document's
//! source bytes and applies every edit as a lossless, path-addressed splice.
//! After each mutation we re-derive the `Value` tree (and the flat rows) from
//! `Editor::source()`, so what's on screen always reflects the canonical bytes.

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use fig::{Document, Editor, Format, Value};

use crate::tree::{self, Row, Seg};

/// Interaction mode: normal navigation, or editing a scalar's text.
pub enum Mode {
    Normal,
    Editing { buffer: String },
}

pub struct App {
    file_path: PathBuf,
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
    pub should_quit: bool,
}

impl App {
    pub fn open(file_path: PathBuf, format: Format) -> Result<Self> {
        let bytes = std::fs::read(&file_path)
            .with_context(|| format!("reading {}", file_path.display()))?;
        let editor = Editor::open(&bytes, format)
            .map_err(|e| anyhow::anyhow!("fig failed to parse the file: {e}"))?;

        let mut app = App {
            file_path,
            format,
            editor,
            value: Value::Null,
            rows: Vec::new(),
            collapsed: HashSet::new(),
            selected: 0,
            mode: Mode::Normal,
            status: "opened".to_string(),
            dirty: false,
            should_quit: false,
        };
        app.reload()?;
        Ok(app)
    }

    pub fn file_name(&self) -> String {
        self.file_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.file_path.display().to_string())
    }

    pub fn format(&self) -> Format {
        self.format
    }

    /// A copy of the editor's current (canonical) source. Handy for tests and
    /// any "show me the raw text" view.
    pub fn source_snapshot(&self) -> String {
        self.editor
            .source()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

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

    // ── navigation ──────────────────────────────────────────────────────────

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
        let Some(row) = self.selected_row() else { return };
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
        let Some(row) = self.selected_row() else { return };
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
        let Some(row) = self.selected_row() else { return };
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

    // ── editing ─────────────────────────────────────────────────────────────

    pub fn begin_edit(&mut self) {
        let Some(row) = self.selected_row() else { return };
        if !row.is_scalar() {
            self.status = "can only edit scalar values".to_string();
            return;
        }
        // Seed the buffer with the current value, resolved from the live tree.
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

        let Some(row) = self.selected_row() else { return };
        let path = row.path.clone();
        let value = tree::parse_scalar(&buffer);

        match self.editor.replace_value(&tree::to_fig(&path), value) {
            Ok(()) => {
                self.after_edit(&path, "value updated");
            }
            Err(e) => {
                // fig rolled the splice back; the document is untouched.
                self.status = format!("rejected: {e}");
            }
        }
    }

    /// `x`: delete the selected mapping entry or sequence item.
    pub fn delete_selected(&mut self) {
        let Some(row) = self.selected_row() else { return };
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
                // The deleted path is gone; aim selection at its parent.
                let parent = path[..path.len() - 1].to_vec();
                self.after_edit(&parent, "deleted");
            }
            Err(e) => self.status = format!("delete failed: {e}"),
        }
    }

    pub fn save(&mut self) {
        match self.editor.source() {
            Ok(src) => match std::fs::write(&self.file_path, src) {
                Ok(()) => {
                    self.dirty = false;
                    self.status = format!("saved {}", self.file_name());
                }
                Err(e) => self.status = format!("save failed: {e}"),
            },
            Err(e) => self.status = format!("save failed: {e}"),
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
                    &entries.iter().find(|(mk, _)| matches!(mk, Value::Str(s) if s == k))?.1
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

    fn sample_app() -> App {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sample.toml");
        App::open(path, Format::Toml).expect("open sample.toml")
    }

    fn select(app: &mut App, path: &[Seg]) {
        app.selected = app
            .rows
            .iter()
            .position(|r| r.path == path)
            .unwrap_or_else(|| panic!("no row for {path:?}"));
    }

    fn type_value(app: &mut App, text: &str) {
        // Replace whatever the edit buffer was seeded with.
        if let Mode::Editing { buffer } = &mut app.mode {
            buffer.clear();
        }
        for c in text.chars() {
            app.edit_push(c);
        }
        app.edit_commit();
    }

    #[test]
    fn edits_a_scalar_losslessly() {
        let mut app = sample_app();

        select(&mut app, &[Seg::Key("version".into())]);
        app.begin_edit();
        type_value(&mut app, "2");

        let src = app.source_snapshot();
        assert!(src.contains("version = 2"), "value changed:\n{src}");
        // Everything untouched stays byte-identical — comments included.
        assert!(src.contains("# the server block"), "comment preserved:\n{src}");
        assert!(src.contains("# flower sample config"), "header preserved:\n{src}");
        assert!(app.dirty);
    }

    #[test]
    fn edits_a_nested_string() {
        let mut app = sample_app();

        select(
            &mut app,
            &[Seg::Key("server".into()), Seg::Key("host".into())],
        );
        app.begin_edit();
        type_value(&mut app, "example.com");

        let src = app.source_snapshot();
        assert!(src.contains("host = \"example.com\""), "nested edit:\n{src}");
        assert!(src.contains("port = 8080"), "sibling untouched:\n{src}");
    }

    #[test]
    fn deletes_a_key() {
        let mut app = sample_app();

        select(&mut app, &[Seg::Key("enabled".into())]);
        app.delete_selected();

        let src = app.source_snapshot();
        assert!(!src.contains("enabled = true"), "key removed:\n{src}");
        assert!(src.contains("title = \"flower\""), "siblings kept:\n{src}");
    }

    #[test]
    fn navigation_folds_and_reanchors() {
        let mut app = sample_app();

        // Collapse [server]; its children should disappear from the row list.
        select(&mut app, &[Seg::Key("server".into())]);
        app.collapse_or_leave();
        assert!(
            !app.rows
                .iter()
                .any(|r| r.path == [Seg::Key("server".into()), Seg::Key("host".into())]),
            "collapsed children hidden"
        );
        // Selection stays on the container we collapsed.
        assert_eq!(app.rows[app.selected].path, [Seg::Key("server".into())]);
    }
}

