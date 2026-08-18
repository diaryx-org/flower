//! flower-ffi — the Swift / C-ABI frontend binding for flower.
//!
//! This is the native-Apple frontend for `flower-core`: it takes the
//! frontend-neutral structural [`Model`] — the navigable tree of config [`Row`]s
//! and the path-addressed, lossless edits routed through fig's editor — and
//! exposes it across a C ABI (via UniFFI) in the shape a SwiftUI outline wants.
//! Core stays the single source of truth for the document; the Swift side only
//! renders [`RowView`]s and forwards navigation / edit intents back in, exactly
//! as the TUI frontend does.
//!
//! ## The boundary is *visible rows*, not the whole tree
//!
//! `flower-core` already flattens the document to the list of rows currently
//! visible (honoring the collapsed set). Every call here returns a [`DocView`]:
//! that flat row list plus the selection, dirty flag, and status — one boundary
//! crossing both mutates and repaints, the same one-shot contract every flower /
//! leaf frontend uses. The SwiftUI side indents each [`RowView`] by its `depth`
//! and draws a twisty for containers; it never walks the tree itself.
//!
//! ## Core owns the model; Swift owns the widgets
//!
//! Structural navigation (into children, out to parents, expand/collapse),
//! type inference on a committed edit, and the lossless splice all live in core.
//! Swift picks the affordance — a disclosure row, an inline text field, a bool
//! toggle — and drives the model by **row index**: the stable coordinate a
//! SwiftUI `List` selection speaks. Each edit is committed through fig's editor,
//! so comments, key order, and formatting the user didn't touch stay byte-for-
//! byte identical.
//!
//! ## Threading
//!
//! A UniFFI object is handed to Swift as a reference-counted handle whose methods
//! take `&self`, so the [`Model`] lives behind a [`Mutex`]. Every call locks,
//! edits or reads, and returns a fresh [`DocView`]. Drive it from the main thread.

use std::sync::{Arc, Mutex};

use fig::Format;
use flower_core::{FigBackend, Model, Seg, VKind};

uniffi::setup_scaffolding!();

/// A failure constructing a document — the only fallible entry point. Every other
/// method operates on an already-parsed model and returns a [`DocView`] directly.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FlowerError {
    /// The `format` string handed to [`FlowerDoc::new`] wasn't one flower knows.
    #[error("unknown format: {name}")]
    UnknownFormat { name: String },
    /// fig failed to parse `source` as the requested format.
    #[error("open error: {message}")]
    Open { message: String },
}

/// One visible line of the config tree — a projection of `flower_core::Row` for
/// the renderer. The SwiftUI side indents by `depth`, draws a disclosure twisty
/// when `is_container`, and shows `preview` as the value (or child count).
#[derive(uniffi::Record, Clone)]
pub struct RowView {
    /// A stable identity for this row — its dotted fig path (`server.limits.port`,
    /// `tags.0`), or `""` for a scalar document. Unique among visible rows, so a
    /// SwiftUI `ForEach`/`List` can key on it.
    pub id: String,
    /// Nesting depth; top-level entries are depth 0.
    pub depth: u32,
    /// The mapping key, or `[i]` for a sequence item.
    pub label: String,
    /// The value kind as a renderer class id: `null`, `bool`, `int`, `float`,
    /// `str`, `ext`, `map`, `seq`. Drives the value colour and which edit widget
    /// a scalar row offers.
    pub kind: String,
    /// A one-line rendering of the value (the scalar text, or `{n}` / `[n]` for a
    /// container). For a scalar this is also the seed text an inline editor opens
    /// with.
    pub preview: String,
    /// A map or sequence (draws a twisty, toggles on tap); the complement is a
    /// scalar (edited in place).
    pub is_container: bool,
    /// Meaningful only for containers: whether it is currently expanded.
    pub expanded: bool,
    /// Whether this row is a mapping entry (its key can be renamed). `false` for a
    /// sequence item, which has an index, not a key.
    pub can_rename: bool,
}

/// A whole rendered frame: the visible rows, which is selected, and the
/// document-level chrome state — everything the SwiftUI side needs for one
/// repaint, in one value. Returned by every method.
#[derive(uniffi::Record)]
pub struct DocView {
    pub rows: Vec<RowView>,
    /// The selected row: an index into [`Self::rows`], clamped to the list.
    pub selected: u32,
    /// Whether the document differs from the last saved bytes — for a "● modified"
    /// affordance and to enable the Save control.
    pub dirty: bool,
    /// The model's one-line status message (last action, or a rejected edit).
    pub status: String,
    /// The document root's kind — `"map"`, `"seq"`, or `"scalar"` — so a frontend
    /// knows whether a top-level "add" inserts a key or appends an item.
    pub root_kind: String,
    /// How many top-level keys are hidden (managed by an embedder). Lets a
    /// frontend show a "N managed fields" affordance.
    pub hidden_count: u32,
}

/// A live flower document bound for a native Apple frontend: a
/// `flower_core::Model` over a [`FigBackend`], behind a mutex. Constructed from
/// in-memory bytes and driven entirely through method calls — there is no
/// filesystem behind it; the host reads the file and persists [`FlowerDoc::source`].
#[derive(uniffi::Object)]
pub struct FlowerDoc {
    inner: Mutex<Inner>,
}

/// The guarded model. A newtype so it can carry the `unsafe impl Send` below;
/// every access goes through [`FlowerDoc::lock`].
struct Inner(Model<FigBackend>);

// SAFETY: `Model<FigBackend>` embeds a `fig::Editor`, which holds a
// `NonNull<FigEditor>` and is therefore `!Send`. UniFFI hands `FlowerDoc` to
// Swift as a reference-counted handle that must be `Send + Sync`, so `Inner` must
// be `Send`. This is sound because:
//   1. Every access goes through `FlowerDoc::lock()` — the `Mutex` serializes all
//      reads and mutations, so there is never concurrent access to the handle.
//   2. fig's editor handle owns a plain heap allocation with no thread-affinity
//      (no thread-locals, no per-thread state) — moving the pointer between
//      threads is fine as long as use is serialized, which (1) guarantees.
// The intended usage is still main-thread-driven; this only permits the handle to
// cross threads safely, it does not invite concurrent use.
unsafe impl Send for Inner {}

impl std::ops::Deref for Inner {
    type Target = Model<FigBackend>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Inner {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[uniffi::export]
impl FlowerDoc {
    /// Parse `source` as `format` (`"json"`/`"jsonc"`/`"json5"`, `"yaml"`/`"yml"`,
    /// `"toml"`, `"zon"`, `"fig"`/`"figl"`) into a live, untitled document.
    ///
    /// `hidden_keys` are **top-level** mapping keys to hide from the row
    /// projection while keeping them in the document (byte-for-byte). Pass `[]`
    /// for a standalone config; a prov/diaryx frontend passes the managed-key set
    /// so those fields stay lossless and out of view. The list is the caller's to
    /// supply — flower stays format-agnostic and never names those keys itself.
    #[uniffi::constructor]
    pub fn new(
        source: String,
        format: String,
        hidden_keys: Vec<String>,
    ) -> Result<Arc<Self>, FlowerError> {
        let format = parse_format(&format)?;
        let backend =
            FigBackend::open(source.as_bytes(), format).map_err(|e| FlowerError::Open {
                message: e.to_string(),
            })?;
        let model = Model::with_hidden(backend, hidden_keys).map_err(|e| FlowerError::Open {
            message: e.to_string(),
        })?;
        Ok(Arc::new(FlowerDoc {
            inner: Mutex::new(Inner(model)),
        }))
    }

    /// Resolve the current document to a renderable frame — the first paint.
    pub fn view(&self) -> DocView {
        view_of(&self.lock())
    }

    /// The canonical serialized document — what the host writes on save.
    pub fn source(&self) -> String {
        self.lock().source_snapshot()
    }

    /// Mark the document saved after the host persisted [`FlowerDoc::source`] its
    /// own way — clears the dirty flag without touching a filesystem.
    pub fn mark_saved(&self) -> DocView {
        let mut m = self.lock();
        m.mark_saved();
        m.set_status("saved");
        view_of(&m)
    }

    // ── selection & navigation ────────────────────────────────────────────────

    /// Select the row at `index` (clamped). The coordinate a `List` selection or a
    /// tap speaks; the other index-taking methods select first, so a caller can
    /// drive purely by index.
    pub fn select(&self, index: u32) -> DocView {
        let mut m = self.lock();
        select_index(&mut m, index);
        view_of(&m)
    }

    pub fn move_up(&self) -> DocView {
        let mut m = self.lock();
        m.move_up();
        view_of(&m)
    }

    pub fn move_down(&self) -> DocView {
        let mut m = self.lock();
        m.move_down();
        view_of(&m)
    }

    /// `→` semantics: expand a collapsed container, else step into its first child.
    pub fn expand_or_enter(&self) -> DocView {
        let mut m = self.lock();
        m.expand_or_enter();
        view_of(&m)
    }

    /// `←` semantics: collapse an expanded container, else step out to the parent.
    pub fn collapse_or_leave(&self) -> DocView {
        let mut m = self.lock();
        m.collapse_or_leave();
        view_of(&m)
    }

    /// Toggle the container at `index` between expanded and collapsed (a
    /// disclosure-triangle tap). Selects the row; a no-op on a scalar row (the
    /// frontend edits those in place instead).
    pub fn toggle(&self, index: u32) -> DocView {
        let mut m = self.lock();
        select_index(&mut m, index);
        if m.rows.get(m.selected).is_some_and(|r| r.is_container()) {
            m.activate();
        }
        view_of(&m)
    }

    // ── editing ───────────────────────────────────────────────────────────────

    /// Commit `text` as the new value of the scalar row at `index`, splicing it
    /// losslessly through fig's editor. The type is the one the schema declares for
    /// that path, or — with no schema, the standalone-config case — a guess from
    /// the literal's shape (`true`/`42`/`3.14`/`null`/text). A no-op on a container
    /// row. The status reflects success or the backend's rejection.
    pub fn set_value(&self, index: u32, text: String) -> DocView {
        let mut m = self.lock();
        select_index(&mut m, index);
        if let Some(row) = m.rows.get(m.selected) {
            if row.is_scalar() {
                let path = row.path.clone();
                m.set_scalar_text(&path, &text);
            } else {
                m.set_status("can only edit scalar values");
            }
        }
        view_of(&m)
    }

    /// Delete the mapping entry or sequence item at `index`, refreshing the view.
    pub fn delete(&self, index: u32) -> DocView {
        let mut m = self.lock();
        select_index(&mut m, index);
        m.delete_selected();
        view_of(&m)
    }

    // ── insert & reorder ──────────────────────────────────────────────────────

    /// Insert `key = text` into the **mapping** at `index`, typing the value by the
    /// schema's rule for the new entry and otherwise by literal shape. A no-op with
    /// a status hint when the row isn't a mapping; the backend rejects a duplicate
    /// key.
    pub fn insert_key(&self, index: u32, key: String, text: String) -> DocView {
        let mut m = self.lock();
        select_index(&mut m, index);
        match m.rows.get(m.selected).map(|r| (r.vkind, r.path.clone())) {
            Some((VKind::Map, path)) => m.insert_key_text(&path, &key, &text),
            Some(_) => m.set_status("select a mapping to add a key"),
            None => {}
        }
        view_of(&m)
    }

    /// Append `text` to the **sequence** at `index`, typing the value by the
    /// schema's rule for the sequence's *items* and otherwise by literal shape. A
    /// no-op with a status hint when the row isn't a sequence.
    pub fn append_item(&self, index: u32, text: String) -> DocView {
        let mut m = self.lock();
        select_index(&mut m, index);
        match m.rows.get(m.selected).map(|r| (r.vkind, r.path.clone())) {
            Some((VKind::Seq, path)) => m.append_item_text(&path, &text),
            Some(_) => m.set_status("select a sequence to add an item"),
            None => {}
        }
        view_of(&m)
    }

    /// Move the row at `index` one place earlier among its siblings.
    pub fn move_row_up(&self, index: u32) -> DocView {
        let mut m = self.lock();
        select_index(&mut m, index);
        m.move_selected_up();
        view_of(&m)
    }

    /// Move the row at `index` one place later among its siblings.
    pub fn move_row_down(&self, index: u32) -> DocView {
        let mut m = self.lock();
        select_index(&mut m, index);
        m.move_selected_down();
        view_of(&m)
    }

    /// Insert `key = text` at the **document root** (a top-level mapping entry),
    /// typed the same way [`insert_key`](Self::insert_key) types one. The
    /// root-level counterpart — there is no root row to target by index. A no-op
    /// with a status hint when the root isn't a mapping.
    pub fn insert_root_key(&self, key: String, text: String) -> DocView {
        let mut m = self.lock();
        if m.root_kind() == "map" {
            m.insert_key_text(&[], &key, &text);
        } else {
            m.set_status("the document root is not a mapping");
        }
        view_of(&m)
    }

    /// Append `text` at the **document root** (a top-level sequence item). The
    /// root-level counterpart to [`append_item`](Self::append_item). A no-op with
    /// a status hint when the root isn't a sequence.
    pub fn append_root_item(&self, text: String) -> DocView {
        let mut m = self.lock();
        if m.root_kind() == "seq" {
            m.append_item_text(&[], &text);
        } else {
            m.set_status("the document root is not a sequence");
        }
        view_of(&m)
    }

    /// Rename the mapping entry at `index` to `new_key`, keeping its value. A
    /// no-op with a status hint when the row is a sequence item (no key); the
    /// backend rejects a name that collides with a sibling.
    pub fn rename_key(&self, index: u32, new_key: String) -> DocView {
        let mut m = self.lock();
        select_index(&mut m, index);
        if let Some(path) = m.rows.get(m.selected).map(|r| r.path.clone()) {
            m.rename_key(&path, &new_key);
        }
        view_of(&m)
    }
}

impl FlowerDoc {
    /// Acquire the guard, recovering from a poisoned lock: a panic in one call
    /// shouldn't wedge the whole document handle for the app.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Clamp `index` to the row list and set it as the selection.
fn select_index(model: &mut Model<FigBackend>, index: u32) {
    if model.rows.is_empty() {
        model.selected = 0;
        return;
    }
    model.selected = (index as usize).min(model.rows.len() - 1);
}

/// Map a format name (the file extension, lowercased) onto a `fig::Format`.
fn parse_format(name: &str) -> Result<Format, FlowerError> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "json" => Format::Json,
        "jsonc" => Format::Jsonc,
        "json5" => Format::Json5,
        "yaml" | "yml" => Format::Yaml,
        "toml" => Format::Toml,
        "zon" => Format::Zon,
        "fig" | "figl" => Format::Fig,
        other => {
            return Err(FlowerError::UnknownFormat {
                name: other.to_string(),
            });
        }
    })
}

/// Snapshot the model as a [`DocView`] — the one place core's row list crosses
/// into the view shape.
fn view_of(model: &Model<FigBackend>) -> DocView {
    let rows = model
        .rows
        .iter()
        .map(|r| RowView {
            id: path_id(&r.path),
            depth: r.depth as u32,
            label: r.label.clone(),
            kind: kind_name(r.vkind).to_string(),
            preview: r.preview.clone(),
            is_container: r.is_container(),
            expanded: r.expanded,
            can_rename: matches!(r.path.last(), Some(Seg::Key(_))),
        })
        .collect();

    DocView {
        rows,
        selected: model.selected as u32,
        dirty: model.dirty,
        status: model.status.clone(),
        root_kind: model.root_kind().to_string(),
        hidden_count: model.hidden_present() as u32,
    }
}

/// The renderer class id for a value kind — kept in sync with the Swift theme.
fn kind_name(k: VKind) -> &'static str {
    match k {
        VKind::Null => "null",
        VKind::Bool => "bool",
        VKind::Int => "int",
        VKind::Float => "float",
        VKind::Str => "str",
        VKind::Ext => "ext",
        VKind::Map => "map",
        VKind::Seq => "seq",
    }
}

/// A stable dotted-path identity for a row: `server.limits.port`, `tags.0`, or
/// `""` for the document root. Unique among visible rows.
fn path_id(path: &[Seg]) -> String {
    let mut s = String::new();
    for seg in path {
        if !s.is_empty() {
            s.push('.');
        }
        match seg {
            Seg::Key(k) => s.push_str(k),
            Seg::Index(i) => s.push_str(&i.to_string()),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
title = \"flower\"
version = 1
enabled = true

[server]
host = \"localhost\"
port = 8080
tags = [\"alpha\", \"beta\"]
";

    fn doc() -> Arc<FlowerDoc> {
        FlowerDoc::new(SAMPLE.to_string(), "toml".to_string(), Vec::new()).unwrap()
    }

    fn row_index(v: &DocView, id: &str) -> u32 {
        v.rows.iter().position(|r| r.id == id).unwrap() as u32
    }

    #[test]
    fn unknown_format_is_reported() {
        let result = FlowerDoc::new("x = 1".to_string(), "ini".to_string(), Vec::new());
        assert!(matches!(
            result.as_ref().map(|_| ()),
            Err(FlowerError::UnknownFormat { .. })
        ));
    }

    #[test]
    fn hidden_keys_are_projected_out_but_kept() {
        let d = FlowerDoc::new(
            SAMPLE.to_string(),
            "toml".to_string(),
            vec!["title".into(), "enabled".into()],
        )
        .unwrap();
        let v = d.view();
        assert!(!v.rows.iter().any(|r| r.id == "title"));
        assert!(!v.rows.iter().any(|r| r.id == "enabled"));
        assert!(v.rows.iter().any(|r| r.id == "version"));
        assert_eq!(v.hidden_count, 2);
        assert_eq!(v.root_kind, "map");
        // Still in the document.
        assert!(d.source().contains("title = \"flower\""));
        assert!(d.source().contains("enabled = true"));
    }

    #[test]
    fn insert_root_key_adds_a_top_level_entry() {
        let d = doc();
        let v = d.insert_root_key("root_flag".to_string(), "true".to_string());
        assert!(v.dirty);
        assert!(v.rows.iter().any(|r| r.id == "root_flag"));
        assert!(d.source().contains("root_flag"));
    }

    #[test]
    fn rename_key_keeps_the_value() {
        let d = doc();
        let i = row_index(&d.view(), "version");
        let v = d.rename_key(i, "revision".to_string());
        assert!(v.dirty);
        assert!(v.rows.iter().any(|r| r.id == "revision"));
        assert!(d.source().contains("revision") && d.source().contains("= 1"));
    }

    #[test]
    fn rename_key_reports_can_rename_flag() {
        let v = doc().view();
        let key_row = v.rows.iter().find(|r| r.id == "version").unwrap();
        let item_row = v.rows.iter().find(|r| r.id == "server.tags.0").unwrap();
        assert!(key_row.can_rename);
        assert!(!item_row.can_rename);
    }

    #[test]
    fn view_lists_top_level_rows() {
        let v = doc().view();
        let labels: Vec<_> = v.rows.iter().map(|r| r.label.as_str()).collect();
        assert!(labels.contains(&"title"));
        assert!(labels.contains(&"server"));
        assert!(!v.dirty);
    }

    #[test]
    fn set_value_edits_losslessly() {
        let d = doc();
        let v = d.view();
        let i = row_index(&v, "version");
        let v = d.set_value(i, "2".to_string());
        assert!(v.dirty);
        assert!(d.source().contains("version = 2"));
        // A comment-free sample, but sibling formatting is preserved by fig.
        assert!(d.source().contains("title = \"flower\""));
    }

    #[test]
    fn set_value_infers_string_for_a_nested_key() {
        let d = doc();
        // `host` is nested under `server`, so its row id is the dotted path.
        let i = row_index(&d.view(), "server.host");
        let v = d.set_value(i, "example.com".to_string());
        assert!(v.dirty);
        assert!(d.source().contains("host = \"example.com\""));
        assert!(d.source().contains("port = 8080"), "sibling untouched");
    }

    #[test]
    fn toggle_collapses_a_container() {
        let d = doc();
        let v = d.view();
        let i = row_index(&v, "server");
        let v = d.toggle(i);
        // Collapsed: server's children are no longer visible.
        assert!(!v.rows.iter().any(|r| r.id == "server.host"));
        // Toggling again re-expands.
        let v = d.toggle(i);
        assert!(v.rows.iter().any(|r| r.id == "server.host"));
    }

    #[test]
    fn append_item_adds_to_a_sequence() {
        let d = doc();
        let i = row_index(&d.view(), "server.tags");
        let v = d.append_item(i, "gamma".to_string());
        assert!(v.dirty);
        assert!(d.source().contains("gamma"));
        assert!(
            v.rows.iter().any(|r| r.id == "server.tags.2"),
            "new item visible"
        );
    }

    #[test]
    fn insert_key_adds_to_a_mapping() {
        let d = doc();
        let i = row_index(&d.view(), "server");
        let v = d.insert_key(i, "scheme".to_string(), "https".to_string());
        assert!(v.dirty);
        assert!(d.source().contains("scheme") && d.source().contains("= \"https\""));
    }

    #[test]
    fn insert_key_on_a_scalar_is_a_hinted_noop() {
        let d = doc();
        let i = row_index(&d.view(), "version");
        let v = d.insert_key(i, "x".to_string(), "1".to_string());
        assert!(!v.dirty);
        assert!(v.status.contains("mapping"));
    }

    #[test]
    fn move_row_reorders_within_the_parent() {
        let d = doc();
        // Move the second tag up; beta should then precede alpha.
        let i = row_index(&d.view(), "server.tags.1");
        d.move_row_up(i);
        let src = d.source();
        assert!(src.find("beta").unwrap() < src.find("alpha").unwrap());
    }

    #[test]
    fn delete_removes_a_key() {
        let d = doc();
        let i = row_index(&d.view(), "enabled");
        d.delete(i);
        assert!(!d.source().contains("enabled = true"));
        assert!(d.source().contains("title = \"flower\""));
    }
}
