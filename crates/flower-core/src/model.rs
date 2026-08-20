//! The frontend-neutral editor model and its structural operations.
//!
//! `Model` is generic over a [`Backend`]: it builds path-addressed [`EditOp`]s,
//! applies them through the backend, and re-derives its view from
//! [`Backend::to_value`] after each change. It owns no editor, no format, no
//! filesystem, and no terminal — the backend owns the document; the embedder
//! owns file I/O and rendering.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use fig::Value;

use crate::backend::{Backend, EditOp};
use crate::page::{self, Page, PageItem};
use crate::schema::{FieldRule, Schema};
use crate::tree::{self, Row, Seg};
use fig_schema::{Issue, SegPat, Validation};

/// Which projection the frontend is navigating: the whole-document
/// [`tree`](crate::tree), or one [`page`](crate::page) at a time.
///
/// The document is unaffected — both are views over the same `Value`, and every
/// edit is path-addressed, so switching mid-session changes what you can see and
/// nothing about what you can do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewMode {
    /// Every visible node at once, indented by depth. Best when the whole
    /// document fits on a screen and you want to read it as a document.
    #[default]
    Tree,
    /// One container at a time, pushed and popped. Best when it doesn't.
    Pages,
}

/// Interaction mode: normal navigation, or editing a scalar's text.
pub enum Mode {
    Normal,
    Editing {
        buffer: String,
        /// The scalar being edited. Held here rather than re-read from the
        /// selection on commit, so an edit belongs to a *node* and not to
        /// whichever list the cursor happens to be in — the two projections
        /// index differently, and a commit must not care which one opened it.
        path: Vec<Seg>,
    },
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
    /// Top-level mapping keys the page projection lists *below* the rest — a
    /// page's "advanced" section (see [`page::PageItem::demoted`]).
    ///
    /// The third answer to "who edits this field?", after `hidden` (something
    /// else does, and its row would only clutter) and `derived` (nothing does).
    /// A demoted key is edited here like any other; it is just not what the
    /// reader came for. Relations, identity, a title the title bar owns: real
    /// fields, worth showing, worth showing last.
    ///
    /// Holds the union with [`derived`](Self::derived), maintained by
    /// [`set_demoted`](Self::set_demoted) — a key nothing can meaningfully edit
    /// is the clearest case there is for sinking it below the ones you can.
    demoted: HashSet<String>,
    /// The schema governing this document, if any — from the backend
    /// ([`Backend::schema`]) or injected by the embedder ([`Model::set_schema`]).
    /// Drives type-directed parsing and commit-time value validation; absent, the
    /// model behaves exactly as before.
    schema: Option<Schema>,

    pub selected: usize,
    pub mode: Mode,
    pub status: String,
    pub dirty: bool,

    // ── page view ─────────────────────────────────────────────────────────
    /// Which projection is being navigated. Both are kept live: the model has no
    /// idea how much width the frontend has, and rebuilding the unused one costs
    /// a walk of a tree that was just rebuilt anyway.
    view: ViewMode,
    /// The container the page view is currently listing. Empty is the root.
    focus: Vec<Seg>,
    /// The page at [`focus`](Self::focus).
    page: Page,
    /// The root's page. Kept for the "is there anything to navigate at all?"
    /// question ([`pages_would_degenerate`](Self::pages_would_degenerate)), which
    /// is about the document rather than about where you are in it.
    root_page: Page,
    /// The page one level out from [`focus`](Self::focus) — the list you were
    /// looking at when you opened the current one.
    ///
    /// A two-pane frontend shows this on the left, so the pair of panes is a
    /// window sliding along the lineage rather than a fixed sidebar: the left is
    /// always the page the right came out of, at every depth.
    parent_page: Page,
    /// The selected item on [`page`](Self::page).
    page_selected: usize,
    /// Where the cursor was on each page we have left, so popping back restores
    /// it rather than dumping you at the top.
    ///
    /// Only a fallback: coming back normally re-finds the child you drilled into,
    /// which survives edits that shift indices. This is what answers when that
    /// child is *gone* — you opened a key and deleted it — and the cursor would
    /// otherwise have nothing to return to.
    page_memory: HashMap<Vec<Seg>, usize>,
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
        Self::with_collapsed(backend, hidden, derived, Vec::new())
    }

    /// Build a model whose containers at `collapsed` arrive **shut**, before the
    /// first row list is ever built.
    ///
    /// A document can have one field nobody reads as a list: an index document's
    /// `contents` is one row per child — ninety-five of them in a year index,
    /// ahead of the four fields anyone types by hand. Such a section wants to open
    /// as a summary, not a wall you scroll past. Toggling it afterwards through
    /// [`activate`](Self::activate) would work, but that is the *interactive*
    /// door: it moves the selection and rebuilds the row list once per container.
    /// Seeding the set here costs neither — the paths are in place before
    /// `reload`, so the opening frame is already correct.
    ///
    /// A path that names a scalar (or nothing at all) is inert rather than an
    /// error, so a caller can name the keys it *wants* collapsed without first
    /// checking which of them turned out to be containers.
    pub fn with_collapsed(
        backend: B,
        hidden: Vec<String>,
        derived: Vec<String>,
        collapsed: Vec<Vec<Seg>>,
    ) -> Result<Self> {
        // The backend supplies the schema when it knows one (a prov backend);
        // otherwise it stays `None` until an embedder injects one.
        let schema = backend.schema();
        let mut model = Model {
            backend,
            value: Value::Null,
            rows: Vec::new(),
            collapsed: collapsed.into_iter().collect(),
            hidden: hidden.into_iter().collect(),
            // Every derived key starts demoted; `set_demoted` adds the
            // embedder's own to that floor rather than replacing it.
            demoted: derived.iter().cloned().collect(),
            derived: derived.into_iter().collect(),
            schema,
            selected: 0,
            mode: Mode::Normal,
            status: "opened".to_string(),
            dirty: false,
            view: ViewMode::default(),
            focus: Vec::new(),
            page: Page::default(),
            root_page: Page::default(),
            parent_page: Page::default(),
            page_selected: 0,
            page_memory: HashMap::new(),
        };
        model.reload()?;
        Ok(model)
    }

    /// Name the top-level keys the page projection sinks below the rest.
    ///
    /// Out-of-band like [`set_schema`](Self::set_schema), and for the same
    /// reason: it is presentation the *embedder* knows and the document does
    /// not. A diaryx host knows `part_of` is drawn by the sidebar and `id` by
    /// nothing at all; the fig-backed model reading the same frontmatter has no
    /// way to tell either from a field somebody typed.
    ///
    /// Adds to the derived keys already demoted rather than replacing them, so a
    /// caller names only what the constructor did not. Rebuilds the pages, so
    /// the next [`page`](Self::page) already reflects it.
    ///
    /// Root keys, matched exactly. A path is demoted when its *first* segment is
    /// one of these, so naming a container demotes everything under it.
    pub fn set_demoted(&mut self, keys: Vec<String>) {
        self.demoted.extend(keys);
        self.rebuild_pages();
    }

    /// Whether the node at `path` sits under a demoted top-level key — the
    /// page projection's own [`is_derived`](Self::is_derived).
    pub fn is_demoted(&self, path: &[Seg]) -> bool {
        matches!(path.first(), Some(Seg::Key(k)) if self.demoted.contains(k))
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
        self.rebuild_pages();
        Ok(())
    }

    fn rebuild_rows(&mut self) {
        self.rows = tree::build_rows(&self.value, &self.collapsed, &self.hidden);
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
    }

    /// Re-derive the focused page and the root page from `value`.
    ///
    /// Runs on every reload, whichever view is active: see
    /// [`view`](Self::view) for why both projections are kept live.
    fn rebuild_pages(&mut self) {
        self.reanchor_focus();
        self.root_page = page::build_page(&self.value, &[], &self.hidden, &self.demoted);
        self.page = if self.focus.is_empty() {
            self.root_page.clone()
        } else {
            page::build_page(&self.value, &self.focus, &self.hidden, &self.demoted)
        };
        self.parent_page = if self.focus.is_empty() {
            Page::default()
        } else {
            page::build_page(
                &self.value,
                &self.focus[..self.focus.len() - 1],
                &self.hidden,
                &self.demoted,
            )
        };
        if self.page_selected >= self.page.items.len() {
            self.page_selected = self.page.items.len().saturating_sub(1);
        }
    }

    /// Walk `focus` back to the nearest ancestor that is still a container.
    ///
    /// The focus is the one piece of page state the document can invalidate from
    /// underneath: delete the key you are standing inside, or replace it with a
    /// scalar, and the page has nothing to list. Popping to the nearest surviving
    /// ancestor is what a settings menu does when a section disappears — you end
    /// up one level out, rather than on a blank page or back at the root.
    fn reanchor_focus(&mut self) {
        while !self.focus.is_empty()
            && !tree::value_at(&self.value, &self.focus).is_some_and(page::is_container)
        {
            self.focus.pop();
        }
    }

    fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    /// The path of whatever is selected in the **active** view.
    ///
    /// The seam that lets one set of edit operations serve both projections: an
    /// edit is a path plus a value, and which list the user picked that path from
    /// is not something [`commit`](Self::commit) should have to know.
    pub fn selected_path(&self) -> Option<Vec<Seg>> {
        match self.view {
            ViewMode::Tree => self.selected_row().map(|r| r.path.clone()),
            ViewMode::Pages => self.page_item().map(|i| i.path.clone()),
        }
    }

    /// Re-anchor selection onto `path` after a rebuild, or clamp if it's gone.
    ///
    /// Re-anchors *both* projections, because an edit made from either one moves
    /// the node in both, and the view the user is not currently looking at is the
    /// one they will switch to expecting their cursor to still be somewhere sane.
    fn select_path(&mut self, path: &[Seg]) {
        if let Some(i) = self.rows.iter().position(|r| r.path == path) {
            self.selected = i;
        } else if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
        // A path off the current page (an edit by path elsewhere in the document,
        // or the anchor of a delete that was the page's own container) leaves the
        // page cursor where it was, clamped by `rebuild_pages`.
        if let Some(i) = self.page.position_of(path) {
            self.page_selected = i;
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

    // ── page view ─────────────────────────────────────────────────────────

    /// Which projection is active.
    pub fn view(&self) -> ViewMode {
        self.view
    }

    /// Switch projection, carrying the cursor across so the node you were on in
    /// one view is the node you are on in the other.
    ///
    /// Without that, switching would be a jump cut: you fold down to one key in
    /// the tree, switch to pages, and land at the top of the root page with no
    /// idea where your key went. Carrying the selection makes the two views two
    /// ways of looking at one position, which is the only reading under which
    /// having both is worth it.
    pub fn set_view(&mut self, view: ViewMode) {
        if view == self.view {
            return;
        }
        let was = self.selected_path();
        self.view = view;
        if let Some(path) = was {
            match view {
                ViewMode::Pages => self.focus_on(&path),
                // The tree may have the node folded away inside a shut ancestor;
                // open the lineage so there is a row to land on.
                ViewMode::Tree => {
                    for i in 0..path.len() {
                        self.collapsed.remove(&path[..i]);
                    }
                    self.rebuild_rows();
                    self.select_path(&path);
                }
            }
        }
    }

    /// Toggle between the tree and the page view.
    pub fn toggle_view(&mut self) {
        self.set_view(match self.view {
            ViewMode::Tree => ViewMode::Pages,
            ViewMode::Pages => ViewMode::Tree,
        });
    }

    /// The page currently being listed.
    pub fn page(&self) -> &Page {
        &self.page
    }

    /// The root's page.
    pub fn root_page(&self) -> &Page {
        &self.root_page
    }

    /// The page one level out — what a two-pane frontend draws on the left. Empty
    /// when [`focus`](Self::focus) is the root, which has no parent.
    pub fn parent_page(&self) -> &Page {
        &self.parent_page
    }

    /// The container the page view is listing. Empty is the document root.
    pub fn focus(&self) -> &[Seg] {
        &self.focus
    }

    /// The index of the selected item on [`page`](Self::page).
    pub fn page_selected(&self) -> usize {
        self.page_selected
    }

    /// The selected page item, if the page has any.
    pub fn page_item(&self) -> Option<&PageItem> {
        self.page.items.get(self.page_selected)
    }

    /// Whether a two-pane layout would waste one pane on this document.
    ///
    /// A document whose root has nothing to drill into — a flat list of keys, a
    /// sequence of scalars — has no navigation to put in a sidebar, and splitting
    /// the width for it would cost half the room and buy nothing. A frontend
    /// checks this to fall back to a single full-width pane.
    pub fn pages_would_degenerate(&self) -> bool {
        !self.root_page.has_drills()
    }

    /// Point the page view at whichever page *lists* `path`, with the cursor on
    /// it — the by-path counterpart to drilling, and how a view switch carries
    /// the selection across.
    ///
    /// It searches from the root outward rather than from `path` inward, because
    /// more than one page can contain a node and the outermost is the right one:
    /// an inlined group's member is listed on the grandparent's page (that is what
    /// inlining means), and also on the group's own page, which is a place page
    /// navigation would never have left you. A path that doesn't resolve is inert.
    pub fn focus_on(&mut self, path: &[Seg]) {
        if tree::value_at(&self.value, path).is_none() {
            return;
        }
        let mut focus: Vec<Seg> = Vec::new();
        while focus.len() < path.len()
            && page::build_page(&self.value, &focus, &self.hidden, &self.demoted)
                .position_of(path)
                .is_none()
        {
            focus.push(path[focus.len()].clone());
        }
        self.focus = focus;
        self.rebuild_pages();
        self.page_selected = self.page.position_of(path).unwrap_or(0);
    }

    /// The page listing the container at `path`, without going there.
    ///
    /// [`page`](Self::page) is where the user *is*; this is any other level, built
    /// on demand and thrown away. A frontend whose navigation is a stack needs it:
    /// the OS asks "what is the screen for this path element?" for levels the
    /// model is not focused on, and answering by moving the focus would make
    /// rendering a screen a navigation.
    ///
    /// Total, like [`build_page`](crate::page::build_page): a path that doesn't
    /// resolve, or that names a scalar, yields an empty page.
    pub fn page_at(&self, path: &[Seg]) -> Page {
        page::build_page(&self.value, path, &self.hidden, &self.demoted)
    }

    /// The page the selected item *would* open.
    ///
    /// A two-pane frontend showing the root's categories on the left has nothing
    /// to put on the right until you have drilled into something — and an empty
    /// half-screen is a poor advertisement for splitting the width. Previewing
    /// the selected category's page fills it with the thing you are about to open
    /// anyway, which is what a settings sidebar does. `None` for a scalar, which
    /// has no page.
    pub fn peek_page(&self) -> Option<Page> {
        let item = self.page_item()?;
        if !item.is_drill() {
            return None;
        }
        Some(self.page_at(&item.path))
    }

    /// `j` in the page view.
    pub fn page_move_down(&mut self) {
        if self.page_selected + 1 < self.page.items.len() {
            self.page_selected += 1;
        }
    }

    /// `k` in the page view.
    pub fn page_move_up(&mut self) {
        self.page_selected = self.page_selected.saturating_sub(1);
    }

    /// `l`/`Enter` in the page view: open the selected container as a page, or
    /// begin editing the selected scalar.
    ///
    /// A group header opens too. Its members are already on screen, so opening it
    /// shows nothing new — but it is the door to operating on the group as a
    /// container (append, insert, reorder) rather than on the members, and a
    /// container that is visible but cannot be entered is a worse surprise than a
    /// page that repeats what you could already see.
    pub fn page_enter(&mut self) {
        let Some(item) = self.page_item() else {
            return;
        };
        if item.is_scalar() {
            self.begin_edit();
            return;
        }
        // A group header opens nothing (see `PageItem::is_drill`), so `l` on one
        // does the next most useful thing and steps onto its first member — the
        // same "into its children" this key means everywhere else.
        if !item.is_drill() {
            if let Some(first) = self.page.items[self.page_selected + 1..]
                .iter()
                .position(|i| i.inset > 0)
            {
                self.page_selected += 1 + first;
            }
            return;
        }
        let target = item.path.clone();
        self.page_memory
            .insert(self.focus.clone(), self.page_selected);
        self.focus = target;
        self.page_selected = 0;
        self.rebuild_pages();
    }

    /// `h`/`Esc` in the page view: pop back to the parent page, restoring the
    /// cursor to the container you came out of.
    pub fn page_back(&mut self) {
        if self.focus.is_empty() {
            self.status = "already at the top".to_string();
            return;
        }
        let child = std::mem::take(&mut self.focus);
        self.focus = child[..child.len() - 1].to_vec();
        self.rebuild_pages();
        // Prefer re-finding the child: an index it holds is correct after edits
        // that shifted the page, which a remembered index would not be. The
        // memory answers only when the child is gone — see `page_memory`.
        self.page_selected = self
            .page
            .position_of(&child)
            .or_else(|| {
                self.page_memory
                    .get(&self.focus)
                    .copied()
                    .filter(|i| *i < self.page.items.len())
            })
            .unwrap_or(0);
    }

    /// Whether the container at `path` is collapsed. Answers for a node with no
    /// row too (one nested inside another collapsed container), which
    /// [`Row::expanded`](crate::Row) cannot.
    pub fn is_collapsed(&self, path: &[Seg]) -> bool {
        self.collapsed.contains(path)
    }

    /// Collapse or expand the container at `path`, leaving the selection where the
    /// user put it — the by-path, non-interactive counterpart to
    /// [`activate`](Self::activate).
    ///
    /// `activate` folds *the selected row*, so driving it from a path means moving
    /// the selection first and putting it back after. This doesn't: it re-anchors
    /// onto whatever was selected before, and only falls back to `path` itself when
    /// the selection was a descendant that the fold just took off screen.
    ///
    /// A path naming a scalar (or nothing) is inert — see
    /// [`with_collapsed`](Self::with_collapsed).
    pub fn set_collapsed(&mut self, path: &[Seg], collapsed: bool) {
        let changed = if collapsed {
            self.collapsed.insert(path.to_vec())
        } else {
            self.collapsed.remove(path)
        };
        if !changed {
            return;
        }
        let was = self.selected_row().map(|r| r.path.clone());
        self.rebuild_rows();
        if let Some(was) = was {
            // A row swallowed by the fold has no path to return to; its nearest
            // surviving ancestor is the container the user just shut.
            if collapsed && was.len() > path.len() && was.starts_with(path) {
                self.select_path(path);
            } else {
                self.select_path(&was);
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
        let Some(path) = self.selected_path() else {
            return;
        };
        let Some(value) = self.value_at(&path) else {
            return;
        };
        if page::is_container(value) {
            self.status = "can only edit scalar values".to_string();
            return;
        }
        let seed = tree::edit_seed(value);
        self.mode = Mode::Editing { buffer: seed, path };
    }

    pub fn edit_push(&mut self, c: char) {
        if let Mode::Editing { buffer, .. } = &mut self.mode {
            buffer.push(c);
        }
    }

    pub fn edit_backspace(&mut self) {
        if let Mode::Editing { buffer, .. } = &mut self.mode {
            buffer.pop();
        }
    }

    pub fn edit_cancel(&mut self) {
        self.mode = Mode::Normal;
        self.status = "edit cancelled".to_string();
    }

    pub fn edit_commit(&mut self) {
        let Mode::Editing { buffer, path } = &mut self.mode else {
            return;
        };
        let buffer = std::mem::take(buffer);
        let path = std::mem::take(path);
        self.mode = Mode::Normal;

        let value = self.coerce_text(&path, &buffer);
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
        let value = self.coerce_text(path, text);
        self.set_value_at(path, value);
    }

    /// Turn edit-buffer `text` into the value that belongs at `path`: the type the
    /// schema declares for that path when it declares one, and otherwise a guess
    /// from the literal's shape.
    ///
    /// The single rule behind [`edit_commit`](Self::edit_commit),
    /// [`set_scalar_text`](Self::set_scalar_text),
    /// [`insert_key_text`](Self::insert_key_text) and
    /// [`append_item_text`](Self::append_item_text). It is keyed on the path of the
    /// value being *written*, not of its container — that is what lets an
    /// each-item rule type a list's items independently of the list.
    fn coerce_text(&self, path: &[Seg], text: &str) -> Value {
        match self.rule_at(path).and_then(|r| r.ty) {
            Some(ty) => ty.coerce(text),
            None => tree::parse_scalar(text),
        }
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

    /// Insert `key = text` into the mapping at `map_path`, coercing `text` by the
    /// type the schema declares for the new entry and otherwise guessing by literal
    /// shape — the insert-shaped analog of
    /// [`set_scalar_text`](Self::set_scalar_text).
    ///
    /// Prefer this to [`insert_key`](Self::insert_key) whenever the value comes
    /// from a user's text: a caller that shape-guesses on its own writes `2026` as
    /// an integer into a field the schema declares `str`, and gets no say from the
    /// schema it is otherwise honoring everywhere else.
    pub fn insert_key_text(&mut self, map_path: &[Seg], key: &str, text: &str) {
        let mut target = map_path.to_vec();
        target.push(Seg::Key(key.to_string()));
        let value = self.coerce_text(&target, text);
        self.insert_key(map_path, key, value);
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

    /// Append `text` to the sequence at `seq_path`, coercing it by the type the
    /// schema declares for the sequence's *items* and otherwise guessing by literal
    /// shape — the append-shaped analog of
    /// [`set_scalar_text`](Self::set_scalar_text).
    ///
    /// The item's type comes from the rule matching the item path (an each-item or
    /// subtree rule), not from the rule on the list itself: `tags` is a `seq`, its
    /// items are `str`.
    pub fn append_item_text(&mut self, seq_path: &[Seg], text: &str) {
        let mut target = seq_path.to_vec();
        target.push(Seg::Index(self.seq_len(seq_path)));
        let value = self.coerce_text(&target, text);
        self.append_item(seq_path, value);
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
        let Some(path) = self.selected_path() else {
            return;
        };
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

    /// The value the document currently holds at `path` (the whole tree for the
    /// empty path), or `None` when the path doesn't resolve — for a frontend
    /// reading a row's value without reaching for the backend.
    pub fn value_at(&self, path: &[Seg]) -> Option<&Value> {
        tree::value_at(&self.value, path)
    }

    /// The mapping keys at `path`, in document order (empty for a non-mapping).
    fn map_keys(&self, path: &[Seg]) -> Vec<String> {
        tree::map_keys(&self.value, path).unwrap_or_default()
    }

    /// The length of the sequence at `path` (0 for a non-sequence) — the index an
    /// append will land at.
    pub fn seq_len(&self, path: &[Seg]) -> usize {
        tree::seq_len(&self.value, path).unwrap_or(0)
    }

    /// `x`: delete the selected mapping entry or sequence item.
    pub fn delete_selected(&mut self) {
        let Some(path) = self.selected_path() else {
            return;
        };
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
        if let Mode::Editing { buffer, .. } = &mut model.mode {
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
        assert!(
            src.contains("# the server block"),
            "comment preserved:\n{src}"
        );
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
        assert!(
            src.contains("host = \"example.com\""),
            "nested edit:\n{src}"
        );
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
        assert!(
            src.contains("alpha") && src.contains("beta"),
            "siblings kept"
        );
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
        assert!(
            !model
                .rows
                .iter()
                .any(|r| r.path == [Seg::Key("title".into())])
        );
        assert!(
            !model
                .rows
                .iter()
                .any(|r| r.path == [Seg::Key("enabled".into())])
        );
        // …but a visible sibling is still there,
        assert!(
            model
                .rows
                .iter()
                .any(|r| r.path == [Seg::Key("version".into())])
        );
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
        assert!(
            title < version && title < enabled,
            "title stayed put:\n{src}"
        );
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
        assert_eq!(
            model.rows[model.selected].path,
            [Seg::Key("revision".into())]
        );
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
        assert!(
            model.status.contains("rejected"),
            "status: {}",
            model.status
        );
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
        let mut model = Model::with_managed(backend, vec!["title".into()], vec!["updated".into()])
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
        assert!(
            out.contains("updated = \"2026-07-01\""),
            "unchanged:\n{out}"
        );

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

    /// The point of a default-collapsed set: the *opening* frame is already
    /// folded, without a toggle pass that walks the selection across the document.
    #[test]
    fn containers_can_arrive_collapsed() {
        let backend = FigBackend::open(SAMPLE.as_bytes(), Format::Toml).expect("open");
        let model = Model::with_collapsed(
            backend,
            Vec::new(),
            Vec::new(),
            vec![
                vec![Seg::Key("server".into())],
                // Naming a scalar is inert, not an error — a caller collapses the
                // keys it means to without first sorting containers from scalars.
                vec![Seg::Key("title".into())],
            ],
        )
        .expect("model");

        let server = model
            .rows
            .iter()
            .find(|r| r.path == [Seg::Key("server".into())])
            .expect("server row");
        assert!(!server.expanded, "collapsed before the first frame");
        assert!(
            !model.rows.iter().any(|r| r.path.len() > 1),
            "no descendant rows: {:?}",
            model.rows.iter().map(|r| &r.label).collect::<Vec<_>>()
        );
        // The inert scalar path didn't cost `title` its row.
        assert!(
            model
                .rows
                .iter()
                .any(|r| r.path == [Seg::Key("title".into())])
        );
        assert_eq!(model.selected, 0, "selection untouched");
    }

    /// Unlike `activate`, folding by path is not a selection move — that is the
    /// whole reason a caller reaches for it.
    #[test]
    fn set_collapsed_folds_by_path_without_moving_the_selection() {
        let mut model = sample_model();
        select(&mut model, &[Seg::Key("title".into())]);

        model.set_collapsed(&[Seg::Key("server".into())], true);
        assert!(model.is_collapsed(&[Seg::Key("server".into())]));
        assert!(
            !model.rows.iter().any(|r| r.path.len() > 1),
            "children hidden"
        );
        assert_eq!(
            model.rows[model.selected].path,
            [Seg::Key("title".into())],
            "selection stayed on title"
        );

        model.set_collapsed(&[Seg::Key("server".into())], false);
        assert!(!model.is_collapsed(&[Seg::Key("server".into())]));
        assert!(
            model
                .rows
                .iter()
                .any(|r| r.path == [Seg::Key("server".into()), Seg::Key("host".into())])
        );
        assert_eq!(model.rows[model.selected].path, [Seg::Key("title".into())]);
    }

    /// The one case where the selection *must* move: it was inside the fold.
    #[test]
    fn set_collapsed_reanchors_a_selection_it_swallowed() {
        let mut model = sample_model();
        select(
            &mut model,
            &[Seg::Key("server".into()), Seg::Key("host".into())],
        );
        model.set_collapsed(&[Seg::Key("server".into())], true);
        assert_eq!(
            model.rows[model.selected].path,
            [Seg::Key("server".into())],
            "landed on the container that swallowed it"
        );
    }

    /// The insert/append counterparts of the type-directed scalar edit: without
    /// them a caller shape-guesses, and `2026` lands in a `str` list as an integer.
    #[test]
    fn insert_and_append_are_type_directed_by_the_schema() {
        use crate::schema::FieldRule;
        use fig_schema::{FieldType, PathPat, Presentation};
        let src = "tags = [\"alpha\"]\n\n[meta]\nk = \"v\"\n";
        let backend = FigBackend::open(src.as_bytes(), Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_schema(crate::schema::Schema::new(vec![
            // The *items* of `tags` are strings — the list itself is a seq.
            FieldRule {
                at: PathPat::each_item_of("tags"),
                ty: Some(FieldType::Str),
                constraint: None,
                present: Presentation::default(),
            },
            FieldRule {
                at: PathPat::key("year"),
                ty: Some(FieldType::Str),
                constraint: None,
                present: Presentation::default(),
            },
            FieldRule {
                at: PathPat(vec![
                    fig_schema::SegPat::Key("meta".into()),
                    fig_schema::SegPat::Key("code".into()),
                ]),
                ty: Some(FieldType::Str),
                constraint: None,
                present: Presentation::default(),
            },
        ]));

        model.append_item_text(&[Seg::Key("tags".into())], "2026");
        model.insert_key_text(&[], "year", "2026");
        // The nested case flower-ffi and Diaryx both shape-guessed.
        model.insert_key_text(&[Seg::Key("meta".into())], "code", "2026");

        let out = model.source_snapshot();
        assert!(
            !out.contains("2026,") && !out.contains("[2026]") && !out.contains("= 2026"),
            "no bare integers survived the schema:\n{out}"
        );
        assert_eq!(
            model.value_at(&[Seg::Key("tags".into()), Seg::Index(1)]),
            Some(&Value::Str("2026".into())),
            "list item took the each-item type:\n{out}"
        );
        assert_eq!(
            model.value_at(&[Seg::Key("year".into())]),
            Some(&Value::Str("2026".into()))
        );
        assert_eq!(
            model.value_at(&[Seg::Key("meta".into()), Seg::Key("code".into())]),
            Some(&Value::Str("2026".into()))
        );
    }

    /// With no rule to consult they fall back to the same shape-guessing the raw
    /// `insert_key`/`append_item` callers do today, so a standalone config is
    /// unaffected.
    #[test]
    fn insert_and_append_text_shape_guess_without_a_schema() {
        let mut model = sample_model();
        model.append_item_text(&[Seg::Key("server".into()), Seg::Key("tags".into())], "42");
        model.insert_key_text(&[], "count", "7");
        assert_eq!(
            model.value_at(&[
                Seg::Key("server".into()),
                Seg::Key("tags".into()),
                Seg::Index(2)
            ]),
            Some(&Value::Int(42))
        );
        assert_eq!(
            model.value_at(&[Seg::Key("count".into())]),
            Some(&Value::Int(7))
        );
    }

    /// The walkers a backend needs, over a plain `Value` — no `Model` in reach.
    #[test]
    fn tree_walkers_resolve_paths_and_reject_mismatches() {
        let model = sample_model();
        let root = model.value_at(&[]).expect("root");

        assert_eq!(
            tree::value_at(root, &[Seg::Key("server".into()), Seg::Key("port".into())]),
            Some(&Value::Int(8080))
        );
        assert_eq!(
            tree::seq_len(root, &[Seg::Key("server".into()), Seg::Key("tags".into())]),
            Some(2)
        );
        // Not a sequence, versus not there at all — both `None`, and neither is a
        // length of zero a caller could mistake for an empty list.
        assert_eq!(tree::seq_len(root, &[Seg::Key("title".into())]), None);
        assert_eq!(tree::seq_len(root, &[Seg::Key("absent".into())]), None);
        assert_eq!(
            tree::map_keys(root, &[Seg::Key("server".into())]),
            Some(vec![
                "host".to_string(),
                "port".to_string(),
                "tags".to_string(),
                "limits".to_string()
            ])
        );
        assert_eq!(tree::map_keys(root, &[Seg::Key("title".into())]), None);
        // A key step into a sequence resolves to nothing rather than guessing.
        assert_eq!(
            tree::value_at(
                root,
                &[
                    Seg::Key("server".into()),
                    Seg::Key("tags".into()),
                    Seg::Key("0".into())
                ]
            ),
            None
        );
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

    // ── the page projection ───────────────────────────────────────────────

    fn key(k: &str) -> Seg {
        Seg::Key(k.to_string())
    }

    /// A model in the page view, cursor on the root page.
    fn paged_model() -> Model<FigBackend> {
        let mut model = sample_model();
        model.set_view(ViewMode::Pages);
        model
    }

    fn page_labels(model: &Model<FigBackend>) -> Vec<String> {
        model.page().items.iter().map(|i| i.label.clone()).collect()
    }

    fn selected_label(model: &Model<FigBackend>) -> String {
        model.page_item().expect("a selected item").label.clone()
    }

    #[test]
    fn drilling_opens_a_page_and_backing_out_returns_the_cursor_to_it() {
        let mut model = paged_model();
        assert!(model.focus().is_empty());

        // Down to `server`, then in.
        for _ in 0..3 {
            model.page_move_down();
        }
        assert_eq!(selected_label(&model), "server");
        model.page_enter();

        assert_eq!(model.focus(), &[key("server")]);
        assert_eq!(selected_label(&model), "host");

        model.page_back();
        assert!(model.focus().is_empty());
        assert_eq!(selected_label(&model), "server");
    }

    #[test]
    fn depth_costs_a_page_not_a_column() {
        let mut model = paged_model();
        // Two levels down, and the page is still four items of one rank plus the
        // members of the groups inlined into it — never an indentation ladder.
        model.focus_on(&[key("server"), key("limits")]);
        assert_eq!(model.focus(), &[key("server")]);
        assert!(model.page().items.iter().all(|i| i.inset <= 1));
        assert_eq!(selected_label(&model), "limits");

        // A group header opens nothing — its members are already here — so `l`
        // steps onto the first of them instead.
        model.page_enter();
        assert_eq!(model.focus(), &[key("server")]);
        assert_eq!(selected_label(&model), "max_connections");
    }

    #[test]
    fn a_group_header_never_opens_a_page_that_repeats_it() {
        let mut model = paged_model();
        model.focus_on(&[key("server")]);
        model.page_enter();
        for header in ["tags", "limits"] {
            let at = model
                .page()
                .items
                .iter()
                .position(|i| i.label == header)
                .expect("the group header");
            assert!(!model.page().items[at].is_drill());
            // Whatever the cursor does, the focused page never becomes the group's.
            model.page_enter();
            assert_eq!(model.focus(), &[key("server")]);
        }
    }

    #[test]
    fn an_edit_made_from_a_page_is_lossless() {
        let mut model = paged_model();
        // An inlined member, two ranks below the page's focus — the case where the
        // page's layout and the document's shape disagree most.
        model.focus_on(&[key("server"), key("limits"), key("timeout")]);
        assert_eq!(model.focus(), &[key("server")]);
        assert_eq!(selected_label(&model), "timeout");

        model.begin_edit();
        type_value(&mut model, "45.5");

        let src = model.source_snapshot();
        assert_eq!(src, SAMPLE.replace("timeout = 30.5", "timeout = 45.5"));
        assert!(model.dirty);
        // The cursor stayed on the field that was edited, in both projections.
        assert_eq!(selected_label(&model), "timeout");
        assert_eq!(
            model.rows[model.selected].path,
            vec![key("server"), key("limits"), key("timeout")]
        );
    }

    #[test]
    fn losing_the_container_you_are_standing_in_pops_you_out() {
        // `b` nests a container, so it is a real drill rather than an inlined
        // group — the only kind of row a page can be opened from.
        let backend =
            FigBackend::open(br#"{"a": {"b": {"c": {"d": 1}}}}"#, Format::Json).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        model.focus_on(&[key("a"), key("b")]);
        model.page_enter();
        assert_eq!(model.focus(), &[key("a"), key("b")]);

        // Replace the container the page is listing with a scalar: the focus now
        // names something that cannot be listed at all.
        model.set_value_at(&[key("a"), key("b")], Value::Int(1));

        assert_eq!(model.focus(), &[key("a")]);
        assert_eq!(page_labels(&model), vec!["b"]);
    }

    #[test]
    fn switching_views_carries_the_selection_both_ways() {
        let mut model = sample_model();
        select(&mut model, &[key("server"), key("limits"), key("timeout")]);

        model.set_view(ViewMode::Pages);
        // The page that *lists* an inlined member is its grandparent's.
        assert_eq!(model.focus(), &[key("server")]);
        assert_eq!(selected_label(&model), "timeout");

        // Move within the page, and the tree lands where the page left off.
        model.page_move_up();
        assert_eq!(selected_label(&model), "max_connections");
        model.set_view(ViewMode::Tree);
        assert_eq!(
            model.rows[model.selected].path,
            vec![key("server"), key("limits"), key("max_connections")]
        );
    }

    #[test]
    fn switching_to_the_tree_opens_the_lineage_of_a_folded_selection() {
        let mut model = sample_model();
        model.set_collapsed(&[key("server")], true);
        model.set_view(ViewMode::Pages);
        model.focus_on(&[key("server"), key("host")]);

        model.set_view(ViewMode::Tree);
        // `server` was shut, so `host` had no row to land on until it was opened.
        assert!(!model.is_collapsed(&[key("server")]));
        assert_eq!(
            model.rows[model.selected].path,
            vec![key("server"), key("host")]
        );
    }

    #[test]
    fn a_flat_document_would_waste_a_second_pane() {
        let flat = FigBackend::open(
            b"a = 1
b = 2
",
            Format::Toml,
        )
        .expect("open");
        let flat = Model::new(flat).expect("model");
        assert!(flat.pages_would_degenerate());
        assert!(!sample_model().pages_would_degenerate());
    }

    #[test]
    fn the_root_page_previews_what_the_cursor_would_open() {
        let mut model = paged_model();
        assert_eq!(selected_label(&model), "title");
        assert!(model.peek_page().is_none(), "a scalar has no page");

        for _ in 0..3 {
            model.page_move_down();
        }
        let peek = model.peek_page().expect("server's page");
        assert_eq!(peek.focus, vec![key("server")]);
        assert_eq!(peek.breadcrumb("‹document›"), "server");
    }

    #[test]
    fn backing_out_past_the_root_is_inert() {
        let mut model = paged_model();
        model.page_back();
        assert!(model.focus().is_empty());
        assert_eq!(model.page_selected(), 0);
    }

    #[test]
    fn the_two_panes_are_consecutive_levels_of_one_lineage() {
        let backend = FigBackend::open(
            br#"{"jobs": {"plan": {"steps": {"a": 1, "b": {"c": 2}}}}}"#,
            Format::Json,
        )
        .expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);

        // At the root there is no parent to show on the left.
        assert!(model.parent_page().is_empty());

        model.page_enter(); // jobs
        assert_eq!(model.parent_page().focus, Vec::<Seg>::new());
        model.page_enter(); // jobs.plan
        assert_eq!(model.parent_page().focus, vec![key("jobs")]);
        model.page_enter(); // jobs.plan.steps
        assert_eq!(model.parent_page().focus, vec![key("jobs"), key("plan")]);

        // The left pane can always mark the row the right one was opened from.
        assert!(model.parent_page().position_of(model.focus()).is_some());
    }
}
