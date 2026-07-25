//! The frontend-neutral editor model and its structural operations.
//!
//! `Model` is generic over a [`Backend`]: it builds path-addressed [`EditOp`]s,
//! applies them through the backend, and re-derives its view from
//! [`Backend::to_value`] after each change. It owns no editor, no format, no
//! filesystem, and no terminal — the backend owns the document; the embedder
//! owns file I/O and rendering.

use std::collections::HashSet;

use anyhow::Result;
use fig::Value;

use crate::backend::{Backend, EditOp};
use crate::schema::{FieldRule, Schema};
use fig_schema::{Issue, SegPat, Validation};
use crate::tree::{self, Row, Seg};

/// Interaction mode: normal navigation, or editing a scalar's text.
pub enum Mode {
    Normal,
    Editing { buffer: String },
}

pub struct Model<B> {
    backend: B,

    /// Derived view state, rebuilt from `backend.to_value()` after every edit.
    value: Value,
    pub rows: Vec<Row>,
    collapsed: HashSet<Vec<Seg>>,
    /// Top-level mapping keys to hide from the row projection (but keep in the
    /// document). Empty for a standalone config; a prov/diaryx embedder passes the
    /// managed-key set so those fields stay lossless and out of view.
    hidden: HashSet<String>,
    /// Top-level mapping keys the *workspace* maintains: shown, but not editable.
    ///
    /// The complement of [`hidden`](Self::hidden), for the other kind of managed
    /// field. A hidden key is edited through some other affordance (a title bar,
    /// a link view) and would only clutter the list; a derived key — a recomputed
    /// timestamp, a content hash — has no other affordance because *nothing*
    /// edits it by hand: the workspace overwrites it on the next write. Hiding
    /// those two alike leaves a user wondering where a field they can see in the
    /// file went, so a derived key keeps its row and declines edits instead.
    derived: HashSet<String>,
    /// The schema governing this document, if any — from the backend
    /// ([`Backend::schema`]) or injected by the embedder ([`Model::set_schema`]).
    /// Drives type-directed parsing and commit-time value validation; absent, the
    /// model behaves exactly as before.
    schema: Option<Schema>,

    pub selected: usize,
    pub mode: Mode,
    pub status: String,
    pub dirty: bool,
}

impl<B: Backend> Model<B> {
    /// Build a model over `backend`.
    pub fn new(backend: B) -> Result<Self> {
        Self::with_hidden(backend, Vec::new())
    }

    /// Build a model that hides the given **top-level** mapping keys from the row
    /// projection while keeping them in the document (see
    /// [`tree::build_rows`](crate::tree::build_rows)). For an embedder whose
    /// format reserves some top-level keys (prov/diaryx-managed frontmatter).
    pub fn with_hidden(backend: B, hidden: Vec<String>) -> Result<Self> {
        Self::with_managed(backend, hidden, Vec::new())
    }

    /// Build a model over `backend` distinguishing the two kinds of managed key:
    /// `hidden` ones produce no row (edited through another affordance), while
    /// `derived` ones keep their row but decline every edit (the workspace
    /// maintains them — see [`derived`](Self::derived)).
    ///
    /// A key in both is hidden: no row means nothing to mark read-only.
    pub fn with_managed(backend: B, hidden: Vec<String>, derived: Vec<String>) -> Result<Self> {
        // The backend supplies the schema when it knows one (a prov backend);
        // otherwise it stays `None` until an embedder injects one.
        let schema = backend.schema();
        let mut model = Model {
            backend,
            value: Value::Null,
            rows: Vec::new(),
            collapsed: HashSet::new(),
            hidden: hidden.into_iter().collect(),
            derived: derived.into_iter().collect(),
            schema,
            selected: 0,
            mode: Mode::Normal,
            status: "opened".to_string(),
            dirty: false,
        };
        model.reload()?;
        Ok(model)
    }

    /// Inject a schema out-of-band — the embedder precedent, mirroring
    /// [`with_hidden`](Self::with_hidden). For a host whose backend does not
    /// supply one but that *knows* the governing schema (a diaryx host feeding a
    /// fig-backed frontmatter block plus its resolved workspace config).
    pub fn set_schema(&mut self, schema: Schema) {
        self.schema = Some(schema);
    }

    /// The schema governing the document, if any.
    pub fn schema(&self) -> Option<&Schema> {
        self.schema.as_ref()
    }

    /// The schema rule governing the node at `path`, if any — for a frontend
    /// deciding a widget (a picker for an enum field) or presentation.
    pub fn rule_at(&self, path: &[Seg]) -> Option<&FieldRule> {
        self.schema.as_ref().and_then(|s| s.rule_for(path))
    }

    /// The kind of the document root, for a frontend deciding how to add a
    /// top-level entry: `"map"`, `"seq"`, or `"scalar"`.
    pub fn root_kind(&self) -> &'static str {
        match self.value {
            Value::Map(_) => "map",
            Value::Seq(_) => "seq",
            _ => "scalar",
        }
    }

    /// How many of the hidden top-level keys are actually present in the document
    /// — for a "N managed fields" affordance.
    pub fn hidden_present(&self) -> usize {
        match &self.value {
            Value::Map(entries) => entries
                .iter()
                .filter(|(k, _)| matches!(k, Value::Str(s) if self.hidden.contains(s)))
                .count(),
            _ => 0,
        }
    }

    /// Whether the node at `path` sits under a workspace-maintained (derived)
    /// top-level key — for a frontend rendering it read-only rather than as an
    /// editable control. Edits to it are declined at the commit funnel regardless.
    pub fn is_derived(&self, path: &[Seg]) -> bool {
        matches!(path.first(), Some(Seg::Key(k)) if self.derived.contains(k))
    }

    /// The schema-declared top-level fields the document does **not** yet carry
    /// — what an "add field" affordance offers, so a declared field is reachable
    /// before it exists.
    ///
    /// Rows are projected from the *document*
    /// ([`build_rows`](crate::tree::build_rows)), so a field the schema declares
    /// but the document omits has no row and is otherwise unreachable: the user
    /// would have to know the key and type it exactly. This closes that gap —
    /// it is the schema's half of the row list, and the reason a declared type
    /// is worth writing down for a field that is empty.
    ///
    /// Only a rule addressing exactly one top-level key names an addable field:
    /// an each-item or subtree rule governs *within* a field rather than naming
    /// one. Hidden (managed) keys are never offered — the embedder reserves
    /// those. Order follows the schema's own rule order, so a caller can present
    /// them as declared.
    pub fn addable_fields(&self) -> Vec<&FieldRule> {
        let Some(schema) = &self.schema else {
            return Vec::new();
        };
        // Only a map root can take a top-level key at all.
        let Value::Map(entries) = &self.value else {
            return Vec::new();
        };
        let present: HashSet<&str> = entries
            .iter()
            .filter_map(|(k, _)| match k {
                Value::Str(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        let mut seen = HashSet::new();
        schema
            .rules()
            .iter()
            .filter(|rule| {
                let [SegPat::Key(name)] = rule.at.0.as_slice() else {
                    return false;
                };
                !present.contains(name.as_str())
                    && !self.hidden.contains(name)
                    && seen.insert(name.as_str())
            })
            .collect()
    }

    /// The canonical serialized document — what the embedder writes on save.
    pub fn source_snapshot(&self) -> String {
        self.backend.source().unwrap_or_default()
    }

    /// The backend, for backend-specific reads (e.g. a prov backend's body).
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// The backend, for backend-specific operations that do **not** change the
    /// metadata tree flower renders (e.g. replacing a prov document's prose
    /// body). An op that *does* change the metadata leaves the view stale — go
    /// through the model's own edit methods for those.
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    /// Clear the dirty flag after the embedder has persisted the source.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    // ── view derivation ───────────────────────────────────────────────────────

    /// Re-derive `value` + `rows` from the backend's current tree.
    fn reload(&mut self) -> Result<()> {
        self.value = self
            .backend
            .to_value()
            .map_err(|e| anyhow::anyhow!("reading value tree: {e}"))?;
        self.rebuild_rows();
        Ok(())
    }

    fn rebuild_rows(&mut self) {
        self.rows = tree::build_rows(&self.value, &self.collapsed, &self.hidden);
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
        // Type-directed parse when the schema knows the field's type (a `str`
        // field keeps `"123"` a string); otherwise fall back to shape-guessing.
        let value = match self.rule_at(&path).and_then(|r| r.ty) {
            Some(ty) => ty.coerce(&buffer),
            None => tree::parse_scalar(&buffer),
        };
        self.commit(
            EditOp::ReplaceValue {
                path: path.clone(),
                value,
            },
            path,
            "value updated",
        );
    }

    /// Programmatically replace the value at `path` (any depth), refreshing the
    /// view. The non-interactive counterpart to [`edit_commit`](Self::edit_commit)
    /// — for an embedder or FFI that edits by path rather than through the
    /// selection.
    pub fn set_value_at(&mut self, path: &[Seg], value: Value) {
        self.commit(
            EditOp::ReplaceValue {
                path: path.to_vec(),
                value,
            },
            path.to_vec(),
            "value updated",
        );
    }

    /// Set the scalar at `path` from an edit-buffer `text`, coercing by the
    /// schema's expected type when known (a `str` field keeps `"123"` a string)
    /// and otherwise guessing by literal shape — the by-path, schema-aware analog
    /// of [`edit_commit`](Self::edit_commit). Validation (closed-vocabulary
    /// rejection) still happens at the commit funnel.
    pub fn set_scalar_text(&mut self, path: &[Seg], text: &str) {
        let value = match self.rule_at(path).and_then(|r| r.ty) {
            Some(ty) => ty.coerce(text),
            None => tree::parse_scalar(text),
        };
        self.set_value_at(path, value);
    }

    /// Rename the mapping entry at `path` to `new_key`, keeping its value and
    /// re-anchoring the selection onto the renamed entry. A no-op (with a status
    /// hint) when `path` doesn't end in a key — a sequence item has no key. The
    /// backend rejects a name that collides with an existing sibling key.
    pub fn rename_key(&mut self, path: &[Seg], new_key: &str) {
        match path.last() {
            Some(Seg::Key(_)) => {
                let mut anchor = path[..path.len() - 1].to_vec();
                anchor.push(Seg::Key(new_key.to_string()));
                self.commit(
                    EditOp::RenameKey {
                        path: path.to_vec(),
                        new_key: new_key.to_string(),
                    },
                    anchor,
                    "renamed",
                );
            }
            _ => self.status = "only mapping keys can be renamed".to_string(),
        }
    }

    /// Insert `key = value` into the mapping at `map_path`, selecting the new
    /// entry. A frontend offers this on a map container; the backend rejects a
    /// duplicate key or a non-mapping target, leaving the document untouched.
    pub fn insert_key(&mut self, map_path: &[Seg], key: &str, value: Value) {
        let mut anchor = map_path.to_vec();
        anchor.push(Seg::Key(key.to_string()));
        self.commit(
            EditOp::InsertKey {
                map_path: map_path.to_vec(),
                key: key.to_string(),
                value,
            },
            anchor,
            "inserted",
        );
    }

    /// Append `value` to the sequence at `seq_path`, selecting the new item.
    pub fn append_item(&mut self, seq_path: &[Seg], value: Value) {
        let idx = self.seq_len(seq_path);
        let mut anchor = seq_path.to_vec();
        anchor.push(Seg::Index(idx));
        self.commit(
            EditOp::AppendItem {
                seq_path: seq_path.to_vec(),
                value,
            },
            anchor,
            "appended",
        );
    }

    /// Move the selected row one place earlier among its siblings — a sequence
    /// item via fig's array-move, a mapping entry via a one-swap reorder.
    pub fn move_selected_up(&mut self) {
        self.reorder_selected(-1);
    }

    /// Move the selected row one place later among its siblings.
    pub fn move_selected_down(&mut self) {
        self.reorder_selected(1);
    }

    /// The shared body of [`move_selected_up`](Self::move_selected_up) /
    /// [`move_selected_down`](Self::move_selected_down): shift the selected row by
    /// `delta` positions within its parent container.
    fn reorder_selected(&mut self, delta: isize) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let path = row.path.clone();
        let Some(last) = path.last().cloned() else {
            self.status = "cannot move the document root".to_string();
            return;
        };
        let parent = path[..path.len() - 1].to_vec();
        match last {
            Seg::Index(i) => {
                let len = self.seq_len(&parent);
                let to = i as isize + delta;
                if to < 0 || to as usize >= len {
                    self.status = "already at the edge".to_string();
                    return;
                }
                let to = to as usize;
                let mut anchor = parent.clone();
                anchor.push(Seg::Index(to));
                self.commit(
                    EditOp::MoveItem {
                        seq_path: parent,
                        from: i,
                        to,
                    },
                    anchor,
                    "moved",
                );
            }
            Seg::Key(k) => {
                let keys = self.map_keys(&parent);
                let Some(pos) = keys.iter().position(|x| *x == k) else {
                    return;
                };
                let target = pos as isize + delta;
                if target < 0 || target as usize >= keys.len() {
                    self.status = "already at the edge".to_string();
                    return;
                }
                let mut order = keys;
                order.swap(pos, target as usize);
                self.commit(
                    EditOp::ReorderKeys {
                        map_path: parent,
                        keys: order,
                    },
                    path,
                    "moved",
                );
            }
        }
    }

    /// The mapping keys at `path`, in document order (empty for a non-mapping).
    fn map_keys(&self, path: &[Seg]) -> Vec<String> {
        match self.value_at_or_root(path) {
            Value::Map(entries) => entries
                .iter()
                .filter_map(|(k, _)| match k {
                    Value::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The length of the sequence at `path` (0 for a non-sequence).
    fn seq_len(&self, path: &[Seg]) -> usize {
        match self.value_at_or_root(path) {
            Value::Seq(items) => items.len(),
            _ => 0,
        }
    }

    /// The value at `path`, or the whole tree for the empty (root) path.
    fn value_at_or_root(&self, path: &[Seg]) -> Value {
        if path.is_empty() {
            self.value.clone()
        } else {
            self.value_at(path).unwrap_or(Value::Null)
        }
    }

    /// `x`: delete the selected mapping entry or sequence item.
    pub fn delete_selected(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let path = row.path.clone();
        let (op, anchor) = match path.last() {
            Some(Seg::Index(i)) => {
                let seq_path = path[..path.len() - 1].to_vec();
                (
                    EditOp::RemoveItem {
                        seq_path: seq_path.clone(),
                        index: *i,
                    },
                    seq_path,
                )
            }
            Some(Seg::Key(_)) => (
                EditOp::DeleteKey { path: path.clone() },
                path[..path.len() - 1].to_vec(),
            ),
            None => {
                self.status = "cannot delete the document root".to_string();
                return;
            }
        };
        self.commit(op, anchor, "deleted");
    }

    /// Apply one edit through the backend, then refresh the view (or report the
    /// rollback). The single path every mutation funnels through — and the choke
    /// point where the schema validates values: a closed vocabulary rejects an
    /// unknown value here, before it reaches the backend; an open one applies but
    /// surfaces a soft warning. fig's reparse stays the last-resort backstop.
    fn commit(&mut self, op: EditOp, anchor: Vec<Seg>, msg: &str) {
        // A workspace-maintained field declines every mutation, not just a value
        // edit: renaming or deleting one would be undone on the next write just
        // as surely as retyping it.
        if let Some(key) = op_root_key(&op)
            && self.derived.contains(key)
        {
            self.status = format!("rejected: `{key}` is maintained by the workspace");
            return;
        }
        let mut warn: Option<Issue> = None;
        if let Some((path, value)) = op_target(&op)
            && let Some(rule) = self.rule_at(&path)
        {
            match rule.validate(value) {
                Validation::Reject(why) => {
                    self.status = format!("rejected: {why}");
                    return;
                }
                Validation::Warn(why) => warn = Some(why),
                Validation::Ok => {}
            }
        }
        match self.backend.apply(op) {
            Ok(()) => {
                self.after_edit(&anchor, msg);
                // A soft-warn overrides the success status so the user sees it.
                if let Some(why) = warn {
                    self.status = why.to_string();
                }
            }
            // The backend rolled back / declined; the document is untouched.
            Err(e) => self.status = format!("rejected: {e}"),
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

/// The (target path, value) a value-bearing [`EditOp`] writes — what schema
/// validation checks. An append's item index isn't known here, so a placeholder
/// `Index(0)` stands in; it only serves to match an `EachItem` rule pattern, which
/// is index-agnostic. Structural ops (delete, move, reorder, rename) carry no new
/// value and return `None`.
/// The top-level mapping key an op would change, if any — the unit at which a
/// document's managed fields are declared, so an edit anywhere beneath one
/// (an item of a managed list, a nested key) is caught along with the field
/// itself.
fn op_root_key(op: &EditOp) -> Option<&str> {
    fn first_key(path: &[Seg]) -> Option<&str> {
        match path.first() {
            Some(Seg::Key(k)) => Some(k.as_str()),
            _ => None,
        }
    }
    match op {
        EditOp::ReplaceValue { path, .. }
        | EditOp::DeleteKey { path }
        | EditOp::RenameKey { path, .. } => first_key(path),
        EditOp::RemoveItem { seq_path, .. }
        | EditOp::AppendItem { seq_path, .. }
        | EditOp::MoveItem { seq_path, .. } => first_key(seq_path),
        // An insert *at the root* names the new top-level key itself; deeper, the
        // container it lands in is what matters.
        EditOp::InsertKey { map_path, key, .. } => match map_path.first() {
            None => Some(key.as_str()),
            _ => first_key(map_path),
        },
        // Reordering the root's own keys moves no field's value.
        EditOp::ReorderKeys { map_path, .. } => first_key(map_path),
    }
}

fn op_target(op: &EditOp) -> Option<(Vec<Seg>, &Value)> {
    match op {
        EditOp::ReplaceValue { path, value } => Some((path.clone(), value)),
        EditOp::InsertKey {
            map_path,
            key,
            value,
        } => {
            let mut p = map_path.clone();
            p.push(Seg::Key(key.clone()));
            Some((p, value))
        }
        EditOp::AppendItem { seq_path, value } => {
            let mut p = seq_path.clone();
            p.push(Seg::Index(0));
            Some((p, value))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::FigBackend;
    use fig::Format;

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

    fn sample_model() -> Model<FigBackend> {
        let backend = FigBackend::open(SAMPLE.as_bytes(), Format::Toml).expect("open backend");
        Model::new(backend).expect("build model")
    }

    fn select(model: &mut Model<FigBackend>, path: &[Seg]) {
        model.selected = model
            .rows
            .iter()
            .position(|r| r.path == path)
            .unwrap_or_else(|| panic!("no row for {path:?}"));
    }

    fn type_value(model: &mut Model<FigBackend>, text: &str) {
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
    fn appends_a_sequence_item() {
        let mut model = sample_model();
        let tags = vec![Seg::Key("server".into()), Seg::Key("tags".into())];
        model.append_item(&tags, Value::Str("gamma".into()));

        let src = model.source_snapshot();
        assert!(src.contains("gamma"), "item appended:\n{src}");
        assert!(src.contains("alpha") && src.contains("beta"), "siblings kept");
        assert!(model.dirty);
    }

    #[test]
    fn inserts_a_mapping_key() {
        let mut model = sample_model();
        let server = vec![Seg::Key("server".into())];
        model.insert_key(&server, "scheme", Value::Str("https".into()));

        let src = model.source_snapshot();
        // fig may quote the inserted key (`"scheme" = …`); both are valid TOML.
        assert!(
            src.contains("scheme") && src.contains("= \"https\""),
            "key inserted:\n{src}"
        );
        assert!(src.contains("host = \"localhost\""), "siblings kept");
    }

    #[test]
    fn moves_a_sequence_item_and_reorders_keys() {
        let mut model = sample_model();

        // Move the second tag ("beta", index 1) up to index 0.
        select(
            &mut model,
            &[
                Seg::Key("server".into()),
                Seg::Key("tags".into()),
                Seg::Index(1),
            ],
        );
        model.move_selected_up();
        let src = model.source_snapshot();
        let a = src.find("alpha").unwrap();
        let b = src.find("beta").unwrap();
        assert!(b < a, "beta now precedes alpha:\n{src}");

        // Move a top-level mapping entry down: title should follow version.
        select(&mut model, &[Seg::Key("title".into())]);
        model.move_selected_down();
        let src = model.source_snapshot();
        assert!(
            src.find("version").unwrap() < src.find("title").unwrap(),
            "version now precedes title:\n{src}"
        );
    }

    #[test]
    fn hidden_top_level_keys_are_projected_out_but_kept_lossless() {
        let backend = FigBackend::open(SAMPLE.as_bytes(), Format::Toml).expect("open");
        let mut model =
            Model::with_hidden(backend, vec!["title".into(), "enabled".into()]).expect("model");

        // Hidden keys produce no rows…
        assert!(!model.rows.iter().any(|r| r.path == [Seg::Key("title".into())]));
        assert!(!model.rows.iter().any(|r| r.path == [Seg::Key("enabled".into())]));
        // …but a visible sibling is still there,
        assert!(model.rows.iter().any(|r| r.path == [Seg::Key("version".into())]));
        // …and the hidden keys remain in the document bytes.
        assert!(model.source_snapshot().contains("title = \"flower\""));
        assert!(model.source_snapshot().contains("enabled = true"));

        // Editing a visible key doesn't disturb the hidden ones.
        select(&mut model, &[Seg::Key("version".into())]);
        model.begin_edit();
        type_value(&mut model, "9");
        let src = model.source_snapshot();
        assert!(src.contains("version = 9"));
        assert!(src.contains("title = \"flower\"") && src.contains("enabled = true"));
    }

    #[test]
    fn reorder_leaves_hidden_keys_in_place() {
        let backend = FigBackend::open(SAMPLE.as_bytes(), Format::Toml).expect("open");
        let mut model = Model::with_hidden(backend, vec!["title".into()]).expect("model");

        // Move a visible top-level key; the hidden `title` must keep its position.
        select(&mut model, &[Seg::Key("enabled".into())]);
        model.move_selected_up(); // enabled moves above version
        let src = model.source_snapshot();
        // title stays first (it was declared before version/enabled).
        let title = src.find("title").unwrap();
        let version = src.find("version").unwrap();
        let enabled = src.find("enabled").unwrap();
        assert!(title < version && title < enabled, "title stayed put:\n{src}");
        assert!(enabled < version, "enabled moved above version:\n{src}");
    }

    #[test]
    fn inserts_a_root_level_key() {
        let mut model = sample_model();
        model.insert_key(&[], "root_flag", Value::Bool(true));
        let src = model.source_snapshot();
        assert!(src.contains("root_flag"), "root key inserted:\n{src}");
        assert!(src.contains("title = \"flower\""), "existing kept");
    }

    #[test]
    fn renames_a_key_losslessly() {
        let mut model = sample_model();
        select(&mut model, &[Seg::Key("version".into())]);
        model.rename_key(&[Seg::Key("version".into())], "revision");
        let src = model.source_snapshot();
        // fig may quote the new key (`"revision" = 1`); both are valid TOML.
        assert!(
            src.contains("revision") && src.contains("= 1"),
            "renamed with value kept:\n{src}"
        );
        assert!(!src.contains("version = 1"), "old key gone");
        // Selection re-anchored onto the renamed entry.
        assert_eq!(model.rows[model.selected].path, [Seg::Key("revision".into())]);
    }

    #[test]
    fn rename_rejects_a_sequence_item() {
        let mut model = sample_model();
        model.rename_key(
            &[
                Seg::Key("server".into()),
                Seg::Key("tags".into()),
                Seg::Index(0),
            ],
            "nope",
        );
        assert!(model.status.contains("mapping keys"));
    }

    #[test]
    fn schema_closed_vocabulary_rejects_an_unknown_edit() {
        use crate::schema::{Constraint, FieldRule};
        use fig_schema::{FieldType, PathPat, Presentation, Term};
        let src = "audience = [\"public\"]\ntitle = \"note\"\n";
        let backend = FigBackend::open(src.as_bytes(), Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_schema(crate::schema::Schema::new(vec![FieldRule {
            at: PathPat::each_item_of("audience"),
            ty: Some(FieldType::Str),
            constraint: Some(Constraint::Enum {
                values: vec![Term::value("public"), Term::value("private")],
                closed: true,
            }),
            present: Presentation::default(),
        }]));

        // An unknown value is rejected at the commit funnel; the document is
        // untouched (fig never sees the edit).
        select(&mut model, &[Seg::Key("audience".into()), Seg::Index(0)]);
        model.begin_edit();
        type_value(&mut model, "familly");
        assert!(model.status.contains("rejected"), "status: {}", model.status);
        assert!(
            model.source_snapshot().contains("public"),
            "document unchanged:\n{}",
            model.source_snapshot()
        );

        // A known value commits normally.
        model.begin_edit();
        type_value(&mut model, "private");
        let out = model.source_snapshot();
        assert!(out.contains("private"), "known value applied:\n{out}");
        assert!(!out.contains("public"), "old value replaced:\n{out}");
    }

    /// A declared field the document omits is otherwise unreachable — it has no
    /// row, because rows come from the document. This is what lets a frontend
    /// offer it.
    #[test]
    fn addable_fields_are_the_declared_keys_the_document_lacks() {
        use crate::schema::{Constraint, FieldRule};
        use fig_schema::{FieldType, PathPat, Presentation, Term};
        let src = "audience = [\"public\"]\ntitle = \"note\"\n";
        let backend = FigBackend::open(src.as_bytes(), Format::Toml).expect("open");
        let mut model =
            Model::with_hidden(backend, vec!["title".into(), "updated".into()]).expect("model");
        model.set_schema(crate::schema::Schema::new(vec![
            // Present in the document — already reachable, so never offered.
            FieldRule {
                at: PathPat::key("audience"),
                ty: Some(FieldType::Str),
                constraint: None,
                present: Presentation::default(),
            },
            // An each-item rule governs *within* a field; it names none.
            FieldRule {
                at: PathPat::each_item_of("audience"),
                ty: Some(FieldType::Str),
                constraint: Some(Constraint::Enum {
                    values: vec![Term::value("public")],
                    closed: true,
                }),
                present: Presentation::default(),
            },
            // Declared, absent, not managed — the one to offer.
            FieldRule {
                at: PathPat::key("created"),
                ty: Some(FieldType::Str),
                constraint: None,
                present: Presentation::default(),
            },
            // Declared and absent, but the embedder manages it.
            FieldRule {
                at: PathPat::key("updated"),
                ty: Some(FieldType::Str),
                constraint: None,
                present: Presentation::default(),
            },
        ]));

        let offered: Vec<_> = model
            .addable_fields()
            .iter()
            .map(|r| match r.at.0.as_slice() {
                [SegPat::Key(k)] => k.clone(),
                _ => unreachable!("only single-key rules are offered"),
            })
            .collect();
        assert_eq!(offered, vec!["created".to_string()]);

        // Once added it is a real row, so it stops being offered.
        model.insert_key(&[], "created", Value::Str("2026-07-24".into()));
        assert!(model.addable_fields().is_empty());
    }

    /// A derived field keeps its row — unlike a hidden one — but declines every
    /// mutation, because the workspace rewrites it on the next save regardless.
    #[test]
    fn a_derived_field_is_visible_but_declines_edits() {
        let src = "title = \"note\"\nupdated = \"2026-07-01\"\ncreated = \"2026-06-01\"\n";
        let backend = FigBackend::open(src.as_bytes(), Format::Toml).expect("open");
        let mut model =
            Model::with_managed(backend, vec!["title".into()], vec!["updated".into()])
                .expect("model");

        // Hidden means no row; derived means a row that is marked.
        let labels: Vec<&str> = model.rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["updated", "created"]);
        assert!(model.is_derived(&[Seg::Key("updated".into())]));
        assert!(!model.is_derived(&[Seg::Key("created".into())]));

        // Every shape of mutation is declined, and the document is untouched.
        model.set_scalar_text(&[Seg::Key("updated".into())], "2026-01-01");
        assert!(model.status.contains("maintained by the workspace"));
        model.rename_key(&[Seg::Key("updated".into())], "modified");
        assert!(model.status.contains("maintained by the workspace"));
        model.selected = 0;
        model.delete_selected();
        assert!(model.status.contains("maintained by the workspace"));
        let out = model.source_snapshot();
        assert!(out.contains("updated = \"2026-07-01\""), "unchanged:\n{out}");

        // A neighbouring ordinary field still edits normally.
        model.set_scalar_text(&[Seg::Key("created".into())], "2026-06-15");
        assert!(model.source_snapshot().contains("2026-06-15"));
    }

    /// Without a schema there is nothing to declare, so nothing is offered —
    /// a standalone config keeps the free-text add path.
    #[test]
    fn addable_fields_are_empty_without_a_schema() {
        let src = "title = \"note\"\n";
        let backend = FigBackend::open(src.as_bytes(), Format::Toml).expect("open");
        let model = Model::new(backend).expect("model");
        assert!(model.addable_fields().is_empty());
    }

    #[test]
    fn schema_typed_field_keeps_a_numeric_string_as_text() {
        use crate::schema::FieldRule;
        use fig_schema::{FieldType, PathPat, Presentation};
        let src = "code = \"x\"\n";
        let backend = FigBackend::open(src.as_bytes(), Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_schema(crate::schema::Schema::new(vec![FieldRule {
            at: PathPat::key("code"),
            ty: Some(FieldType::Str),
            constraint: None,
            present: Presentation::default(),
        }]));

        select(&mut model, &[Seg::Key("code".into())]);
        model.begin_edit();
        type_value(&mut model, "123");
        // Schema says `str`, so the buffer stays a quoted string rather than being
        // coerced to an integer the way the shape-guessing heuristic would.
        let out = model.source_snapshot();
        assert!(out.contains("code = \"123\""), "kept as string:\n{out}");
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
