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
//! ## Two projections, one document
//!
//! The tree is not the only shape core offers. [`FlowerDoc::show_pages`] switches
//! to the **page** projection — one container at a time, with small all-scalar
//! groups inlined, pushed and popped like a settings menu — and returns a
//! [`PagesView`]: the page you are on, the page it came out of, and the page the
//! cursor would open, so a two-pane host needs one crossing per frame there too.
//!
//! The page methods address nodes by the dotted-path `id` a [`RowView`] already
//! carries rather than by index, because a page item need not be a visible row at
//! all — its ancestors may be folded away in the tree — and because a page host
//! holds three panes' worth of ids at once. Both projections are the same
//! document and the same path-addressed edits; switching carries the cursor
//! across, so they are two ways of looking at one position.
//!
//! ## Threading
//!
//! A UniFFI object is handed to Swift as a reference-counted handle whose methods
//! take `&self`, so the [`Model`] lives behind a [`Mutex`]. Every call locks,
//! edits or reads, and returns a fresh [`DocView`]. Drive it from the main thread.

use std::sync::{Arc, Mutex};

use fig::{Format, Value};
use flower_core::page::{self, Page, PageItem};
use flower_core::{FigBackend, ItemKind, Model, Seg, VKind, ViewMode};

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

// ── the page projection ──────────────────────────────────────────────────────

/// One line of a page — the projection of `flower_core::PageItem`.
///
/// Where a [`RowView`] is a line of the whole document indented by `depth`, this
/// is a line of *one container's* listing: no depth, because everything on a page
/// is one level of the same thing, and an [`inset`](Self::inset) of 1 only for the
/// members of a group inlined into it.
#[derive(uniffi::Record, Clone)]
pub struct PageItemView {
    /// The item's dotted fig path — the same identity a [`RowView`] carries, so
    /// the two projections name a node the same way and every page method takes
    /// this string.
    pub id: String,
    /// The mapping key, or `[i]` for a sequence item.
    pub label: String,
    /// A readable stand-in for a sequence item's index — the value of whichever
    /// field best names it. Shown *beside* the label, never instead of it: the
    /// index is what the path addresses and what a reorder moves.
    pub title: Option<String>,
    /// The value kind as a renderer class id — the same vocabulary
    /// [`RowView::kind`] uses.
    pub kind: String,
    /// What activating this row does: `"scalar"` (edit in place), `"drill"` (open
    /// a page of its own), or `"group"` (the header of a container inlined into
    /// this page — its members are the rows below it at `inset` 1).
    pub role: String,
    /// A one-line rendering of the value (the scalar text, or `{n}` / `[n]`). For
    /// a scalar this is also the seed text an inline editor opens with.
    pub preview: String,
    /// A container's whole contents in flow form (`{branches: [master]}`), when
    /// they are short enough to be worth showing instead of counting. A renderer
    /// prefers this over `count` whenever the row has room for it — `1 field ›`
    /// is strictly less than the document says when the field is right there.
    pub summary: Option<String>,
    /// How many children a container holds; 0 for a scalar.
    pub count: u32,
    /// 0 for a direct child of the page's focus; 1 for a member of a group inlined
    /// into it. Never more.
    pub inset: u32,
    /// Whether this is a mapping entry (its key can be renamed).
    pub can_rename: bool,
}

/// A step of a page's breadcrumb: the container it names, and the id to open it.
#[derive(uniffi::Record, Clone)]
pub struct CrumbView {
    pub id: String,
    pub label: String,
}

/// One page, ready to render as a pane.
#[derive(uniffi::Record, Clone)]
pub struct PageView {
    /// The dotted path of the container being listed; `""` is the document root.
    pub focus: String,
    /// The trail from the root down to [`focus`](Self::focus), each step openable
    /// by its `id`. **Empty at the root**, whose name is the frontend's to choose
    /// — flower-core has none for it (the TUI calls it `‹document›`).
    pub crumbs: Vec<CrumbView>,
    pub items: Vec<PageItemView>,
    /// The selected item, or `None` for a pane that doesn't hold the cursor — a
    /// preview, or the parent pane when what it marks is the page you opened.
    pub selected: Option<u32>,
}

/// A whole rendered frame of the page view: the pane you are on, the pane it came
/// out of, and the one it would open — plus the same document chrome a
/// [`DocView`] carries, so a page-view host needs nothing else.
#[derive(uniffi::Record)]
pub struct PagesView {
    /// The page currently being listed, with the cursor on it.
    pub page: PageView,
    /// The page one level out — what a two-pane layout draws on the left, with
    /// the row you came out of marked. `None` at the root, which has no parent.
    pub parent: Option<PageView>,
    /// The page the selection *would* open, so a two-pane layout at the root has
    /// something to put on the right before anything has been opened. `None` when
    /// the selection is not a drill row.
    pub peek: Option<PageView>,
    /// Whether a two-pane layout is worth drawing at all: false for a document
    /// whose root has nothing to drill into, where the second pane would cost half
    /// the width and show nothing.
    pub two_pane: bool,
    pub dirty: bool,
    pub status: String,
    pub root_kind: String,
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

    // ── the page projection ───────────────────────────────────────────────────
    //
    // The page methods drive the other projection: one container at a time, with
    // small all-scalar groups inlined, pushed and popped. They address nodes by
    // the same dotted-path `id` a `RowView` carries — not by index — because a
    // page item need not be a visible row at all (its ancestors may be folded in
    // the tree), and because a page host has three panes' worth of ids in hand.
    //
    // Each one puts the model in the page view first. The model keeps both
    // projections live, but *which is active* decides which selection an edit
    // reads, so a page method that quietly operated in tree mode would edit
    // whatever the tree cursor happened to be on.

    /// Switch to the page view and resolve it to a renderable frame.
    ///
    /// The switch carries the cursor across: the node selected in the tree is the
    /// node selected on the page you land on, so the two views are two ways of
    /// looking at one position rather than two places.
    pub fn show_pages(&self) -> PagesView {
        let mut m = self.lock();
        m.set_view(ViewMode::Pages);
        pages_of(&m)
    }

    /// Switch back to the tree view, carrying the cursor the same way.
    pub fn show_tree(&self) -> DocView {
        let mut m = self.lock();
        m.set_view(ViewMode::Tree);
        view_of(&m)
    }

    /// The page frame as it stands, without switching projection — for a host that
    /// keeps both surfaces rendered and needs to refresh the one an edit was not
    /// made from.
    pub fn pages(&self) -> PagesView {
        pages_of(&self.lock())
    }

    /// Put the cursor on the item `id` names, pointing the page view at whichever
    /// page *lists* it. A tap on a row.
    ///
    /// `""` is the document root, which selects nothing and lists the root page.
    pub fn page_select(&self, id: String) -> PagesView {
        let mut m = self.lock();
        m.set_view(ViewMode::Pages);
        if let Some(path) = path_for_id(&m, &id) {
            m.focus_on(&path);
        }
        pages_of(&m)
    }

    /// Open what `id` names as a page — a tap on a drill row, on a row in the
    /// pane you came out of, or on a breadcrumb. `""` opens the document root.
    ///
    /// A scalar or a group header has no page of its own, so this only selects it
    /// (a group's members are already listed under it; a scalar is edited in
    /// place through [`page_set_value`](Self::page_set_value)).
    pub fn page_open(&self, id: String) -> PagesView {
        let mut m = self.lock();
        m.set_view(ViewMode::Pages);
        if let Some(path) = path_for_id(&m, &id) {
            m.focus_on(&path);
            if m.page_item().is_some_and(PageItem::is_drill) {
                m.page_enter();
            }
        }
        pages_of(&m)
    }

    /// The page listing what `id` names, **without navigating to it** — `""` for
    /// the document root.
    ///
    /// The read-only counterpart to [`page_open`](Self::page_open), for a frontend
    /// whose navigation is a stack: the OS asks it to render the screen for a path
    /// element that is not the one in front, and answering that by moving the
    /// focus would make drawing a screen a navigation. It carries the cursor only
    /// when `id` *is* the page you are on; an ancestor is a page you are not
    /// standing on, and drawing a selection there would claim otherwise.
    ///
    /// Total: an id that names nothing, or names a scalar, yields an empty page,
    /// so a destination builder always has something to render.
    pub fn page_at(&self, id: String) -> PageView {
        let m = self.lock();
        let Some(path) = path_for_id(&m, &id) else {
            return PageView {
                focus: id,
                crumbs: Vec::new(),
                items: Vec::new(),
                selected: None,
            };
        };
        let selected = (path == m.focus()).then(|| m.page_selected());
        page_view_of(&m.page_at(&path), selected)
    }

    /// Pop back to the parent page, restoring the cursor to the container you came
    /// out of.
    pub fn page_back(&self) -> PagesView {
        let mut m = self.lock();
        m.set_view(ViewMode::Pages);
        m.page_back();
        pages_of(&m)
    }

    /// Move the cursor one item up the current page (`↑`).
    pub fn page_move_up(&self) -> PagesView {
        let mut m = self.lock();
        m.set_view(ViewMode::Pages);
        m.page_move_up();
        pages_of(&m)
    }

    /// Move the cursor one item down the current page (`↓`).
    pub fn page_move_down(&self) -> PagesView {
        let mut m = self.lock();
        m.set_view(ViewMode::Pages);
        m.page_move_down();
        pages_of(&m)
    }

    // ── editing from a page ───────────────────────────────────────────────────

    /// Commit `text` as the new value of the scalar `id` names, spliced losslessly
    /// through fig — the page-view peer of [`set_value`](Self::set_value).
    pub fn page_set_value(&self, id: String, text: String) -> PagesView {
        let mut m = self.lock();
        m.set_view(ViewMode::Pages);
        match path_for_id(&m, &id) {
            Some(path) if m.value_at(&path).is_some_and(|v| !page::is_container(v)) => {
                m.focus_on(&path);
                m.set_scalar_text(&path, &text);
            }
            Some(_) => m.set_status("can only edit scalar values"),
            None => {}
        }
        pages_of(&m)
    }

    /// Delete the mapping entry or sequence item `id` names.
    pub fn page_delete(&self, id: String) -> PagesView {
        let mut m = self.lock();
        m.set_view(ViewMode::Pages);
        if let Some(path) = path_for_id(&m, &id) {
            m.focus_on(&path);
            m.delete_selected();
        }
        pages_of(&m)
    }

    /// Rename the mapping entry `id` names, keeping its value. A no-op with a
    /// status hint on a sequence item, which has an index, not a key.
    pub fn page_rename(&self, id: String, new_key: String) -> PagesView {
        let mut m = self.lock();
        m.set_view(ViewMode::Pages);
        if let Some(path) = path_for_id(&m, &id) {
            m.rename_key(&path, &new_key);
        }
        pages_of(&m)
    }

    /// Add a child to the container `id` names: `key = text` for a mapping, an
    /// appended `text` for a sequence. `""` is the document root.
    ///
    /// One method for both because a page names its own container by id — adding
    /// "to this page" and adding "to that row" are the same call with a different
    /// id, which is not true of the tree, where the root has no row.
    pub fn page_add_child(&self, id: String, key: String, text: String) -> PagesView {
        let mut m = self.lock();
        m.set_view(ViewMode::Pages);
        if let Some(path) = path_for_id(&m, &id) {
            match m.value_at(&path) {
                Some(Value::Map(_)) => m.insert_key_text(&path, &key, &text),
                Some(Value::Seq(_)) => m.append_item_text(&path, &text),
                _ => m.set_status("can only add to a mapping or a sequence"),
            }
        }
        pages_of(&m)
    }

    /// Move the item `id` names one place earlier among its siblings.
    pub fn page_move_item_up(&self, id: String) -> PagesView {
        self.reorder_from_page(&id, -1)
    }

    /// Move the item `id` names one place later among its siblings.
    pub fn page_move_item_down(&self, id: String) -> PagesView {
        self.reorder_from_page(&id, 1)
    }
}

impl FlowerDoc {
    /// Acquire the guard, recovering from a poisoned lock: a panic in one call
    /// shouldn't wedge the whole document handle for the app.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The shared body of the two page reorders: put the cursor on `id`, then
    /// shift it among its siblings. Reordering is a selection-based operation in
    /// core (it is "move *this* one", not "move the thing at this path to that
    /// path"), so the page methods select first and let core do the rest.
    fn reorder_from_page(&self, id: &str, delta: i32) -> PagesView {
        let mut m = self.lock();
        m.set_view(ViewMode::Pages);
        if let Some(path) = path_for_id(&m, id) {
            m.focus_on(&path);
            if delta < 0 {
                m.move_selected_up();
            } else {
                m.move_selected_down();
            }
        }
        pages_of(&m)
    }
}

/// Clamp `index` to the row list and set it as the selection — and put the model
/// in the tree view, which is the projection an index means anything in.
///
/// The switch is not bookkeeping. Core resolves a selection-based op (delete,
/// reorder) against whichever projection is *active*, so an index-taking method
/// called while the model was left in the page view would edit whatever the page
/// cursor was on. Every index-taking method establishes its selection here, so
/// this is the one place that has to say so — the page methods say the same thing
/// the other way round.
fn select_index(model: &mut Model<FigBackend>, index: u32) {
    model.set_view(ViewMode::Tree);
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

/// Snapshot the model's page state as a [`PagesView`] — the page projection's
/// peer of [`view_of`], and the only place `flower_core::Page` crosses into the
/// view shape.
///
/// Which pane holds the cursor is decided here rather than in Swift, because it
/// follows from the model: the page you are on always has it; the pane you came
/// out of marks the row you opened, which is a trace and not a selection; a peek
/// is a preview of something not yet opened, so it has none at all.
fn pages_of(model: &Model<FigBackend>) -> PagesView {
    let page = page_view_of(model.page(), Some(model.page_selected()));
    let parent = (!model.focus().is_empty()).then(|| {
        let parent = model.parent_page();
        page_view_of(parent, parent.position_of(model.focus()))
    });
    let peek = model.peek_page().map(|p| page_view_of(&p, None));

    PagesView {
        page,
        parent,
        peek,
        two_pane: !model.pages_would_degenerate(),
        dirty: model.dirty,
        status: model.status.clone(),
        root_kind: model.root_kind().to_string(),
        hidden_count: model.hidden_present() as u32,
    }
}

fn page_view_of(page: &Page, selected: Option<usize>) -> PageView {
    PageView {
        focus: path_id(&page.focus),
        crumbs: crumbs_of(page),
        items: page.items.iter().map(item_view_of).collect(),
        selected: selected.map(|i| i as u32),
    }
}

/// The trail from the root down to a page's focus, each step openable by id.
///
/// The last step takes the page's own title when it has one, so a sequence item
/// reads as what it is rather than as `[2]` — the same substitution
/// `Page::breadcrumb` makes, and the same name the row carried on the page you
/// opened it from.
fn crumbs_of(page: &Page) -> Vec<CrumbView> {
    let last = page.focus.len().saturating_sub(1);
    page.focus
        .iter()
        .enumerate()
        .map(|(i, seg)| CrumbView {
            id: path_id(&page.focus[..=i]),
            label: match (i == last, &page.title) {
                (true, Some(title)) => title.clone(),
                _ => page::seg_label(seg),
            },
        })
        .collect()
}

fn item_view_of(item: &PageItem) -> PageItemView {
    let (role, count) = match item.kind {
        ItemKind::Scalar => ("scalar", 0),
        ItemKind::Drill { count } => ("drill", count),
        ItemKind::GroupHeader { count } => ("group", count),
    };
    PageItemView {
        id: path_id(&item.path),
        label: item.label.clone(),
        title: item.title.clone(),
        kind: kind_name(item.vkind).to_string(),
        role: role.to_string(),
        preview: item.preview.clone(),
        summary: item.summary.clone(),
        count: count as u32,
        inset: item.inset as u32,
        can_rename: matches!(item.path.last(), Some(Seg::Key(_))),
    }
}

/// The fig path a page-view `id` names.
///
/// Resolved by *looking it up* among the pages the model has live rather than by
/// parsing the dotted string back into segments: `tags.0` is a sequence index and
/// `limits.0` could be a key spelled `0`, and a key may contain a dot — a printed
/// path is a display form, not a parseable one. Every id a frontend can hold came
/// from one of these panes, so the lookup is total for anything it can send. The
/// empty id is the document root, which no item names.
fn path_for_id(model: &Model<FigBackend>, id: &str) -> Option<Vec<Seg>> {
    if id.is_empty() {
        return Some(Vec::new());
    }
    let on = |page: &Page| {
        page.items
            .iter()
            .find(|i| path_id(&i.path) == id)
            .map(|i| i.path.clone())
    };
    on(model.page())
        .or_else(|| on(model.parent_page()))
        .or_else(|| on(model.root_page()))
        .or_else(|| model.peek_page().as_ref().and_then(on))
        .or_else(|| ancestor_of_focus(model.focus(), id))
}

/// The step of the current lineage `id` names, if it names one.
///
/// The panes above hold the ids of *items* — things listed on a page — and a
/// breadcrumb's middle steps are not among them: the page you are on lists its
/// own children, the pane beside it lists its siblings, and the root lists the
/// top of the trail, but nothing live lists the levels in between. They are
/// prefixes of the focus by construction, which is also true of every element of
/// a navigation stack's path, so that is where they resolve.
fn ancestor_of_focus(focus: &[Seg], id: &str) -> Option<Vec<Seg>> {
    (1..=focus.len())
        .map(|n| &focus[..n])
        .find(|prefix| path_id(prefix) == id)
        .map(<[Seg]>::to_vec)
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

    // ── the page projection ───────────────────────────────────────────────────

    /// Deep enough to have somewhere to drill: a nested mapping, a small group
    /// that inlines, and a sequence of mappings the titles have to name.
    const DEEP: &str = "\
name: flower
server:
  host: localhost
  port: 8080
  limits:
    max_connections: 100
    timeout: 30
jobs:
  - name: build
    runs_on: macos
  - name: test
    runs_on: linux
";

    fn deep() -> Arc<FlowerDoc> {
        FlowerDoc::new(DEEP.to_string(), "yaml".to_string(), Vec::new()).unwrap()
    }

    fn ids(page: &PageView) -> Vec<&str> {
        page.items.iter().map(|i| i.id.as_str()).collect()
    }

    fn item<'p>(page: &'p PageView, id: &str) -> &'p PageItemView {
        page.items.iter().find(|i| i.id == id).unwrap()
    }

    #[test]
    fn the_root_page_lists_one_level() {
        let v = deep().show_pages();
        assert_eq!(ids(&v.page), ["name", "server", "jobs"]);
        assert_eq!(item(&v.page, "name").role, "scalar");
        assert_eq!(item(&v.page, "server").role, "drill");
        assert_eq!(item(&v.page, "jobs").count, 2);
        assert!(v.page.crumbs.is_empty(), "the root has no trail");
        assert!(v.parent.is_none(), "and no page it came out of");
        assert!(v.two_pane, "there is something to drill into");
    }

    #[test]
    fn a_flat_document_asks_for_one_pane() {
        let d =
            FlowerDoc::new("a = 1\nb = 2\n".to_string(), "toml".to_string(), Vec::new()).unwrap();
        assert!(!d.show_pages().two_pane);
    }

    #[test]
    fn opening_a_drill_pushes_a_page_and_back_pops_it() {
        let d = deep();
        d.show_pages();
        let v = d.page_open("server".to_string());
        assert_eq!(v.page.focus, "server");
        assert_eq!(
            v.page
                .crumbs
                .iter()
                .map(|c| c.label.as_str())
                .collect::<Vec<_>>(),
            ["server"]
        );
        // The left pane is the page it was opened from, marking the row it came
        // out of — a trace, not a second cursor.
        let parent = v.parent.expect("a pushed page has a parent pane");
        assert_eq!(parent.focus, "");
        assert_eq!(parent.selected, Some(1));

        let v = d.page_back();
        assert_eq!(v.page.focus, "");
        assert_eq!(v.page.selected, Some(1), "the cursor returns to `server`");
    }

    #[test]
    fn a_small_group_is_inlined_with_its_members_under_it() {
        let d = deep();
        d.show_pages();
        let v = d.page_open("server".to_string());
        assert_eq!(
            ids(&v.page),
            [
                "server.host",
                "server.port",
                "server.limits",
                "server.limits.max_connections",
                "server.limits.timeout",
            ]
        );
        assert_eq!(item(&v.page, "server.limits").role, "group");
        assert_eq!(item(&v.page, "server.limits.timeout").inset, 1);
        // A group header opens no page of its own — its members are already here.
        let v = d.page_open("server.limits".to_string());
        assert_eq!(v.page.focus, "server", "still on the page listing it");
    }

    #[test]
    fn sequence_items_are_named_by_what_is_in_them() {
        let d = deep();
        d.show_pages();
        let v = d.page_open("jobs".to_string());
        let first = item(&v.page, "jobs.0");
        assert_eq!(first.label, "[0]", "the index is what the path addresses");
        assert_eq!(first.title.as_deref(), Some("build"));
        assert_eq!(item(&v.page, "jobs.1").title.as_deref(), Some("test"));
    }

    #[test]
    fn a_small_container_shows_its_contents_rather_than_a_count() {
        let d = FlowerDoc::new(
            "[on.push]\nbranches = [\"master\"]\n".to_string(),
            "toml".to_string(),
            Vec::new(),
        )
        .unwrap();
        let v = d.show_pages();
        assert_eq!(
            item(&v.page, "on").summary.as_deref(),
            Some("{push: {branches: [master]}}")
        );
    }

    #[test]
    fn editing_from_a_page_is_lossless() {
        let d = deep();
        d.show_pages();
        d.page_open("server".to_string());
        let v = d.page_set_value("server.port".to_string(), "9090".to_string());
        assert!(v.dirty);
        assert!(d.source().contains("port: 9090"));
        assert!(d.source().contains("host: localhost"), "sibling untouched");
    }

    #[test]
    fn editing_a_container_from_a_page_is_a_hinted_noop() {
        let d = deep();
        let v = d.show_pages();
        assert!(!v.dirty);
        let v = d.page_set_value("server".to_string(), "nope".to_string());
        assert!(!v.dirty);
        assert!(v.status.contains("scalar"));
    }

    #[test]
    fn a_page_adds_to_the_container_it_is_listing() {
        let d = deep();
        d.show_pages();
        let v = d.page_open("server".to_string());
        assert_eq!(v.page.focus, "server");
        // The page names its own container, so "add to this page" is the same
        // call as "add to that row".
        let v = d.page_add_child(
            v.page.focus.clone(),
            "scheme".to_string(),
            "https".to_string(),
        );
        assert!(v.page.items.iter().any(|i| i.id == "server.scheme"));
        assert!(d.source().contains("scheme"));
    }

    #[test]
    fn a_page_appends_to_a_sequence_it_names() {
        let d = deep();
        d.show_pages();
        let v = d.page_add_child("jobs".to_string(), String::new(), "third".to_string());
        assert!(v.dirty);
        assert!(d.source().contains("third"));
    }

    #[test]
    fn deleting_and_reordering_from_a_page_address_the_row_tapped() {
        let d = deep();
        d.show_pages();
        d.page_open("jobs".to_string());
        d.page_move_item_up("jobs.1".to_string());
        let src = d.source();
        assert!(src.find("test").unwrap() < src.find("build").unwrap());

        let v = d.page_delete("jobs.0".to_string());
        assert!(!d.source().contains("test"));
        assert!(v.page.items.iter().any(|i| i.id == "jobs.0"), "one left");
    }

    #[test]
    fn renaming_from_a_page_keeps_the_value() {
        let d = deep();
        d.show_pages();
        d.page_open("server".to_string());
        let v = d.page_rename("server.host".to_string(), "hostname".to_string());
        assert!(v.page.items.iter().any(|i| i.id == "server.hostname"));
        assert!(d.source().contains("localhost"));
    }

    #[test]
    fn switching_projections_carries_the_cursor_both_ways() {
        let d = deep();
        let i = row_index(&d.view(), "server.limits.timeout");
        d.select(i);
        // The tree cursor lands on the page that *lists* that key — the group is
        // inlined into `server`, so that is `server`, not `server.limits`.
        let v = d.show_pages();
        assert_eq!(v.page.focus, "server");
        assert_eq!(
            ids(&v.page)[v.page.selected.unwrap() as usize],
            "server.limits.timeout"
        );

        let v = d.show_tree();
        assert_eq!(v.rows[v.selected as usize].id, "server.limits.timeout");
    }

    #[test]
    fn the_root_page_hides_the_keys_the_tree_hides() {
        let d = FlowerDoc::new(
            DEEP.to_string(),
            "yaml".to_string(),
            vec!["name".to_string()],
        )
        .unwrap();
        let v = d.show_pages();
        assert_eq!(ids(&v.page), ["server", "jobs"]);
        assert_eq!(v.hidden_count, 1);
        assert!(d.source().contains("name: flower"), "still in the file");
    }

    #[test]
    fn the_selection_previews_the_page_it_would_open() {
        let d = deep();
        d.show_pages();
        let v = d.page_select("server".to_string());
        let peek = v.peek.expect("a drill row previews its page");
        assert_eq!(peek.focus, "server");
        assert_eq!(peek.selected, None, "a preview holds no cursor");
        // A scalar has no page to preview.
        assert!(d.page_select("name".to_string()).peek.is_none());
    }

    #[test]
    fn an_index_op_after_a_page_op_still_means_a_row() {
        let d = deep();
        // Leave the model in the page view, deep in the document…
        d.show_pages();
        d.page_open("server".to_string());
        d.page_select("server.port".to_string());
        // …then delete by row index. The index names a *row*, so it must resolve
        // against the tree, not against whatever the page cursor was on.
        let i = row_index(&d.view(), "name");
        d.delete(i);
        assert!(!d.source().contains("name: flower"));
        assert!(
            d.source().contains("port: 8080"),
            "the page cursor was not the target"
        );
    }

    /// Three drillable levels, so a breadcrumb has a step that is on no live pane.
    const NESTED: &str = "\
a:
  b:
    c:
      one: 1
      two: 2
      more:
        x: 1
    other:
      p: 1
      q: 2
  b2:
    p: 1
";

    #[test]
    fn a_middle_breadcrumb_opens_the_page_it_names() {
        let d = FlowerDoc::new(NESTED.to_string(), "yaml".to_string(), Vec::new()).unwrap();
        d.show_pages();
        d.page_open("a".to_string());
        d.page_open("a.b".to_string());
        let v = d.page_open("a.b.c".to_string());
        assert_eq!(v.page.focus, "a.b.c", "three levels down");
        assert_eq!(
            v.page
                .crumbs
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "a.b", "a.b.c"]
        );

        // Tapping the middle crumb must go back up to it. That id is on no live
        // pane — this page lists `c`'s fields, the parent lists `b`'s, and the
        // root lists `a` — so it resolves as a prefix of the focus, which is what
        // a crumb always is.
        let v = d.page_open("a.b".to_string());
        assert_eq!(v.page.focus, "a.b");
    }

    #[test]
    fn page_at_renders_a_level_without_going_there() {
        let d = FlowerDoc::new(NESTED.to_string(), "yaml".to_string(), Vec::new()).unwrap();
        d.show_pages();
        d.page_open("a".to_string());
        d.page_open("a.b".to_string());
        let v = d.page_open("a.b.c".to_string());
        assert_eq!(v.page.focus, "a.b.c");

        // Every level of the trail renders, including the ones no live pane holds.
        for (id, first) in [("", "a"), ("a", "a.b"), ("a.b", "a.b.c")] {
            let page = d.page_at(id.to_string());
            assert_eq!(page.focus, id);
            assert_eq!(page.items.first().map(|i| i.id.as_str()), Some(first));
            assert_eq!(page.selected, None, "an ancestor holds no cursor");
        }
        // …and the focus is still where the user left it.
        assert_eq!(d.pages().page.focus, "a.b.c");

        // The page you *are* on keeps its cursor.
        assert_eq!(d.page_at("a.b.c".to_string()).selected, Some(0));
    }

    #[test]
    fn page_at_is_total_for_an_id_that_names_nothing() {
        let d = deep();
        d.show_pages();
        let page = d.page_at("nope.not.here".to_string());
        assert!(page.items.is_empty());
        assert_eq!(page.selected, None);
    }
}
