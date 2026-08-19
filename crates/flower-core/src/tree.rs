//! The navigable view of a config document: a flat list of `Row`s derived from
//! fig's `Value` tree.
//!
//! Each row carries its **fig path** — a `Vec<Seg>` of mapping keys and sequence
//! indices from the document root. That path is exactly what `fig::Editor`'s ops
//! take (`&[fig::Segment]`), so navigation and editing speak the same language:
//! move the selection to a row, then hand its path straight to
//! `replace_value` / `delete` / `remove_item` / … .
//!
//! Flattening (rather than bough's recursive path-arithmetic over a nested tree)
//! keeps j/k navigation a single index step and naturally handles the fact that
//! a config path interleaves keys and indices.

use std::collections::HashSet;

use fig::Value;
pub use fig_schema::Seg;

/// Borrow an owned path as fig's `Segment` slice for an editor call.
pub fn to_fig(path: &[Seg]) -> Vec<fig::Segment<'_>> {
    path.iter()
        .map(|s| match s {
            Seg::Key(k) => fig::Segment::Key(k.as_str()),
            Seg::Index(i) => fig::Segment::Index(*i),
        })
        .collect()
}

/// The node a fig path names in `root`, or `None` when the path doesn't resolve.
/// The empty path is the root itself.
///
/// The one walk from a path to its node. Public because `Model` is not the only
/// thing that needs it: a [`Backend`](crate::Backend) lowering an op it has no
/// native primitive for has to read the current tree, and it can't see the model
/// (the model owns *it*). A frontend resolving a row's value by path is the other
/// caller. Both would otherwise reimplement this traversal.
pub fn value_at<'v>(root: &'v Value, path: &[Seg]) -> Option<&'v Value> {
    let mut cur = root;
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
    Some(cur)
}

/// The length of the sequence at `path`. `None` when the path doesn't resolve or
/// names something that isn't a sequence — the two cases a caller sizing an
/// append index or a reorder permutation has to tell apart from an empty list.
pub fn seq_len(root: &Value, path: &[Seg]) -> Option<usize> {
    match value_at(root, path)? {
        Value::Seq(items) => Some(items.len()),
        _ => None,
    }
}

/// The mapping keys at `path`, in document order. `None` when the path doesn't
/// resolve or names something that isn't a mapping.
///
/// Non-string keys are skipped: the editor addresses entries by name, so a key
/// it cannot name is one it cannot reorder or rename.
pub fn map_keys(root: &Value, path: &[Seg]) -> Option<Vec<String>> {
    match value_at(root, path)? {
        Value::Map(entries) => Some(
            entries
                .iter()
                .filter_map(|(k, _)| match k {
                    Value::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    }
}

/// The value kind of a row, for styling and container logic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VKind {
    Null,
    Bool,
    Int,
    Float,
    Str,
    Ext,
    Map,
    Seq,
}

impl VKind {
    pub(crate) fn of(v: &Value) -> Self {
        match v {
            Value::Null => VKind::Null,
            Value::Bool(_) => VKind::Bool,
            Value::Int(_) | Value::Uint(_) => VKind::Int,
            Value::Float(_) => VKind::Float,
            Value::Str(_) => VKind::Str,
            Value::Extended { .. } => VKind::Ext,
            Value::Map(_) => VKind::Map,
            Value::Seq(_) => VKind::Seq,
        }
    }
}

/// One visible line of the tree.
#[derive(Clone, Debug)]
pub struct Row {
    /// Nesting depth (top-level entries are depth 0).
    pub depth: usize,
    /// The mapping key, or `[i]` for a sequence item.
    pub label: String,
    pub vkind: VKind,
    /// A one-line rendering of the value (the scalar text, or `{n}` / `[n]`).
    pub preview: String,
    /// Meaningful only for containers: whether it is currently expanded.
    pub expanded: bool,
    /// The fig path to this node from the document root.
    pub path: Vec<Seg>,
}

impl Row {
    pub fn is_container(&self) -> bool {
        matches!(self.vkind, VKind::Map | VKind::Seq)
    }

    /// A scalar can be edited in place; a container cannot.
    pub fn is_scalar(&self) -> bool {
        !self.is_container()
    }
}

/// Render a mapping key `Value` as a display string.
pub(crate) fn key_to_string(k: &Value) -> String {
    match k {
        Value::Str(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Bool(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

/// A compact one-line preview of a value.
pub fn preview(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => s.clone(),
        Value::Extended { text, .. } => text.clone(),
        Value::Map(entries) => format!("{{{}}}", entries.len()),
        Value::Seq(items) => format!("[{}]", items.len()),
    }
}

/// The editable text a scalar starts with when you enter edit mode.
pub fn edit_seed(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        other => preview(other),
    }
}

/// Build the flat row list from a document root, honoring the collapsed set
/// (paths whose containers are collapsed) and a set of **top-level** mapping keys
/// to hide.
///
/// Hiding is scoped to the root map's own entries — a nested key that happens to
/// share a hidden name is untouched. The hidden entries stay in the underlying
/// `Value` (and therefore in the document's bytes); they merely produce no row.
/// This is how a consumer whose format reserves some top-level keys (e.g. prov's
/// managed frontmatter — `id`, `prov`, `contents`, …) keeps them lossless while
/// showing the user only their own fields. Because only the *projection* is
/// filtered — never the `Value` itself — sibling reorders still see the full key
/// order and leave the hidden keys in place.
pub fn build_rows(
    root: &Value,
    collapsed: &HashSet<Vec<Seg>>,
    hidden_top_level: &HashSet<String>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    match root {
        // A map/seq root shows its children at depth 0 (no synthetic root row).
        Value::Map(entries) => {
            for (k, v) in entries {
                let key = key_to_string(k);
                if hidden_top_level.contains(&key) {
                    continue;
                }
                push_node(
                    &key,
                    v,
                    vec![Seg::Key(key.clone())],
                    0,
                    collapsed,
                    &mut rows,
                );
            }
        }
        Value::Seq(items) => {
            for (i, v) in items.iter().enumerate() {
                push_node(
                    &format!("[{i}]"),
                    v,
                    vec![Seg::Index(i)],
                    0,
                    collapsed,
                    &mut rows,
                );
            }
        }
        // A scalar (or empty/null) document is a single row.
        other => push_node("", other, Vec::new(), 0, collapsed, &mut rows),
    }
    rows
}

fn push_node(
    label: &str,
    v: &Value,
    path: Vec<Seg>,
    depth: usize,
    collapsed: &HashSet<Vec<Seg>>,
    rows: &mut Vec<Row>,
) {
    let vkind = VKind::of(v);
    let is_container = matches!(vkind, VKind::Map | VKind::Seq);
    let expanded = is_container && !collapsed.contains(&path);

    rows.push(Row {
        depth,
        label: label.to_string(),
        vkind,
        preview: preview(v),
        expanded,
        path: path.clone(),
    });

    if expanded {
        match v {
            Value::Map(entries) => {
                for (k, child) in entries {
                    let key = key_to_string(k);
                    let mut p = path.clone();
                    p.push(Seg::Key(key.clone()));
                    push_node(&key, child, p, depth + 1, collapsed, rows);
                }
            }
            Value::Seq(items) => {
                for (i, child) in items.iter().enumerate() {
                    let mut p = path.clone();
                    p.push(Seg::Index(i));
                    push_node(&format!("[{i}]"), child, p, depth + 1, collapsed, rows);
                }
            }
            _ => {}
        }
    }
}

/// Parse an edit-buffer string into a `Value`, inferring type by literal shape.
/// A prototype heuristic — a schema layer would instead pick the type the key
/// expects and validate against it. fig's editor reparse is the backstop: a
/// value that can't splice validly is rejected and rolled back.
pub fn parse_scalar(s: &str) -> Value {
    let t = s.trim();
    match t {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" => return Value::Null,
        _ => {}
    }
    if let Ok(i) = t.parse::<i64>() {
        return Value::Int(i);
    }
    if let Ok(u) = t.parse::<u64>() {
        return Value::Uint(u);
    }
    if let Ok(f) = t.parse::<f64>() {
        return Value::Float(f);
    }
    Value::Str(s.to_string())
}
