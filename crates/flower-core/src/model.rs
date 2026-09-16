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

use crate::annotate::{self, Annotation};
use crate::backend::{Backend, EditOp};
use crate::page::{self, InlineBudget, Page, PageItem};
use crate::schema::{self, Choice, FieldRule, FieldRuleExt, Schema};
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

/// Interaction mode: normal navigation, or editing one text field of a node.
pub enum Mode {
    Normal,
    /// Picking a value off a list rather than typing one
    /// ([`Model::begin_choose`]).
    ///
    /// A mode of its own rather than an editor seeded with a list, because the
    /// two take different keys: everything printable is a *filter* here and a
    /// value there, and `Enter` commits a row rather than a buffer.
    Choosing {
        /// The node being chosen for.
        path: Vec<Seg>,
        /// Everything on offer, unfiltered and in the order it was offered.
        choices: Vec<Choice>,
        /// Which of the *filtered* choices the cursor is on — see
        /// [`Model::visible_choices`].
        selected: usize,
        /// What has been typed to narrow the list.
        filter: String,
    },
    Editing {
        buffer: String,
        /// The node being edited. Held here rather than re-read from the
        /// selection on commit, so an edit belongs to a *node* and not to
        /// whichever list the cursor happens to be in — the two projections
        /// index differently, and a commit must not care which one opened it.
        path: Vec<Seg>,
        /// Which of the node's texts the buffer holds.
        slot: EditSlot,
    },
}

/// The text of a node an inline editor can hold: its value, or one of the two
/// comments fig anchors to it.
///
/// One editor, three targets, because they are typed the same way — a buffer
/// in a footer, `Enter` to commit — and differ only in what the commit writes.
/// A frontend that draws a different affordance per slot (a multi-line box for
/// a leading block) reads this to choose it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditSlot {
    /// The scalar's value, coerced by shape (or by schema) on commit.
    Value,
    /// The own-line comment block above the node. May hold newlines; an empty
    /// buffer commits as *no comment*, removing the block.
    LeadingComment,
    /// The same-line comment after the value. Single-line; an empty buffer
    /// removes it.
    TrailingComment,
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
    /// What the host has to say about particular nodes — host state, re-attached
    /// to the rows on every rebuild ([`annotate`](crate::annotate)).
    annotations: Vec<Annotation>,
    /// The schema governing this document, if any — from the backend
    /// ([`Backend::schema`]) or injected by the embedder ([`Model::set_schema`]).
    /// Drives type-directed parsing and commit-time value validation; absent, the
    /// model behaves exactly as before.
    schema: Option<Schema>,

    /// The selected row of the **tree** projection — an index into
    /// [`rows`](Self::rows), and meaningless against a page.
    ///
    /// Private, and the one piece of cursor state that is. A row index only says
    /// what it means in the projection it was read from, and a public field
    /// cannot check which projection a caller is in — so writing it goes through
    /// [`select_row`](Self::select_row), which can.
    selected: usize,
    pub mode: Mode,
    /// The last thing that happened worth saying out loud — almost always a
    /// refusal (`rejected: ...`, `only mapping keys can be renamed`).
    ///
    /// Empty until something happens. A frontend draws this in whatever it uses
    /// for a status line, and an empty string is what lets it draw *nothing*:
    /// a bar that opens holding a word nobody asked for teaches the reader to
    /// stop reading it, which is the one thing a refusal channel cannot afford.
    pub status: String,
    pub dirty: bool,

    // ── page view ─────────────────────────────────────────────────────────
    /// Which projection is being navigated. Both are kept live: the model has no
    /// idea how much width the frontend has, and rebuilding the unused one costs
    /// a walk of a tree that was just rebuilt anyway.
    view: ViewMode,
    /// How much of a container's subtree the page projection inlines rather
    /// than drills ([`page::InlineBudget`]). The default is the settings-menu
    /// rule; an embedder that knows its room raises it
    /// ([`set_inline_budget`](Self::set_inline_budget)).
    inline_budget: InlineBudget,
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

    // ── history ───────────────────────────────────────────────────────────
    /// The edits made, newest last, each with the ops that undo it — the
    /// journal [`undo`](Self::undo) walks back through.
    undo_stack: Vec<Change>,
    /// The edits undone, newest last, each replayable by [`redo`](Self::redo).
    /// Cleared by the next fresh [`commit`](Self::commit).
    redo_stack: Vec<Change>,
    /// A number that goes up on every successful commit, undo and redo — see
    /// [`edit_seq`](Self::edit_seq).
    edit_seq: u64,
    /// The source as of the last save (or the open), against which
    /// [`dirty`](Self::dirty) is recomputed. Undoing back to it reads as clean,
    /// which is why the flag is derived from the bytes rather than from how
    /// deep the journal is.
    saved_source: String,
}

/// What the page view was pointing at, by identity, at the moment an edit was
/// applied — see [`Model::identities`].
///
/// A path addresses a sequence item by position, so a reorder or a delete of an
/// earlier sibling re-points every path after it. Holding the *keys* alongside
/// the indices is what lets the model put the cursor and the open page back on
/// the item they were on rather than on whatever has since taken its number.
struct Identities {
    /// One entry per segment of the focus.
    focus: Vec<Option<String>>,
    /// The remembered cursors, each with the keys of its own path.
    memory: Vec<(Vec<Seg>, Vec<Option<String>>)>,
}

/// One committed edit, with what it takes to undo it and to do it again.
///
/// The inverse is a *list* because two ops in [`EditOp`]'s vocabulary do not
/// invert to one: restoring a deleted mapping entry is an insert (which
/// appends) plus the reorder that puts it back where it was, and restoring a
/// removed sequence item is an append plus a move. Its comments ride along the
/// same way — fig anchors a comment to the node, so setting it after the insert
/// and before the reorder leaves it attached through both.
#[derive(Debug, Clone)]
struct Change {
    /// The op as committed — what [`Model::redo`] replays.
    forward: EditOp,
    /// The ops that put the document back, in order.
    inverse: Vec<EditOp>,
    /// Where the cursor was anchored by the commit, so undoing puts the row
    /// that changes back on screen.
    anchor: Vec<Seg>,
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
            annotations: Vec::new(),
            selected: 0,
            mode: Mode::Normal,
            // Nothing has happened yet, so there is nothing to report. See
            // `status`.
            status: String::new(),
            dirty: false,
            view: ViewMode::default(),
            inline_budget: InlineBudget::default(),
            focus: Vec::new(),
            page: Page::default(),
            root_page: Page::default(),
            parent_page: Page::default(),
            page_selected: 0,
            page_memory: HashMap::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            edit_seq: 0,
            saved_source: String::new(),
        };
        model.reload()?;
        // The bytes the document opened with are the baseline `dirty` is
        // measured against, until the embedder saves and moves it.
        model.saved_source = model.source_snapshot();
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

    /// Set how much of a container's subtree the page projection inlines rather
    /// than drills.
    ///
    /// Out-of-band like [`set_demoted`](Self::set_demoted), and for the same
    /// reason: the right amount is a fact about the *room* the pages are drawn
    /// in — a frontmatter panel wants the whole document on one page, a narrow
    /// pane over a deep config wants a page per level — and only the embedder
    /// knows which it is. Rebuilds the pages, so the next
    /// [`page`](Self::page) already reflects it.
    ///
    /// The cursor stays on the row it was on, by path rather than by index — a
    /// budget is the one setting that changes how many rows a page has, so the
    /// index under the cursor is exactly what it invalidates. Raising the budget
    /// far enough turns row three of a list into the third field of its first
    /// entry, and a reader who resized a window did not ask to be moved.
    pub fn set_inline_budget(&mut self, budget: InlineBudget) {
        let was_on = self.page_item().map(|i| i.path.clone());
        self.inline_budget = budget;
        self.rebuild_pages();
        if let Some(path) = was_on
            && let Some(i) = self.page.position_of(&path)
        {
            self.page_selected = i;
        }
    }

    /// The inline budget the page projection is currently built with.
    pub fn inline_budget(&self) -> InlineBudget {
        self.inline_budget
    }

    /// Set the inline budget from the room a page actually has
    /// ([`InlineBudget::fitting`]) — `room` being how many rows a frontend can
    /// draw items into, once its own chrome has taken what it needs.
    ///
    /// [`set_inline_budget`](Self::set_inline_budget) is the embedder deciding;
    /// this is the embedder *measuring*, which is the same decision made against
    /// the one fact that turns out to settle it. A frontend that can be resized
    /// calls this whenever the room changes, which is cheap to do every frame:
    /// the pages are rebuilt only when the answer moves.
    pub fn fit_to_room(&mut self, room: usize) {
        let budget = InlineBudget::fitting(&self.value, room);
        if budget != self.inline_budget {
            self.set_inline_budget(budget);
        }
    }

    /// Open the document at the first page that says something.
    ///
    /// A document whose root holds one container — `{repo: [...]}`, and every
    /// file that is one list under one key — has a root page with a single drill
    /// row on it, naming the thing you are obviously about to open. That is the
    /// same page [`PageItem::descend_to`] exists to skip, arrived at from
    /// outside rather than from a row, and the reasons match: it costs a
    /// navigation step to be told the name of the file you just opened.
    ///
    /// Called once, by the frontend, after the budget is set — it is a decision
    /// about where to *start*, not a property of the projection, and re-running
    /// it on every rebuild would take the root page away from a reader who had
    /// pressed `h` to reach it. Nothing is lost either way: the root page is one
    /// step out, and the row's own ops are the ops of the container this lands
    /// in.
    pub fn enter_document(&mut self) {
        while self.page.items.len() == 1 && self.page.items[0].is_drill() {
            self.focus = self.page.items[0].descend_to.clone();
            self.page_selected = 0;
            self.rebuild_pages();
        }
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

    /// Hand the model the host's findings about this document, replacing
    /// whatever it was given last — a broken link, a duplicate id, a rule only
    /// a workspace can check ([`annotate`](crate::annotate)).
    ///
    /// Out-of-band like [`set_schema`](Self::set_schema) and
    /// [`set_demoted`](Self::set_demoted), and for the same reason: flower-core
    /// is one document with no filesystem, so it can never compute one of
    /// these. They are re-attached to the rows on every rebuild, so an edit
    /// does not wipe the markers out from under a reader — but nothing here
    /// re-checks them either, so a host refreshes after a save (or whenever its
    /// own check finishes) by calling this again. An empty list clears them.
    pub fn set_annotations(&mut self, annotations: Vec<Annotation>) {
        self.annotations = annotations;
        self.rebuild_rows();
        self.rebuild_pages();
    }

    /// The findings the host last supplied, in the order it gave them.
    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// The finding that applies at `path`: the one addressed exactly at it, or
    /// failing that the one at its nearest annotated ancestor.
    ///
    /// The inheriting answer, for a caller *asking about a node* — an item of a
    /// list is in trouble when the list is. The rows carry the exact answer
    /// instead ([`PageItem::annotation`](crate::PageItem::annotation)), because
    /// a marker that came down a subtree would point at every row but the one
    /// that is wrong.
    pub fn annotation_at(&self, path: &[Seg]) -> Option<&Annotation> {
        annotate::applying_at(&self.annotations, path)
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
    ///
    /// Moves the baseline [`dirty`](Self::dirty) is measured against to the
    /// bytes just written, and leaves the journal alone: a save is not a
    /// history boundary, so undo still runs back through it — and undoing to
    /// the saved text reads as clean again, because the flag is a comparison
    /// and not a count.
    pub fn mark_saved(&mut self) {
        self.saved_source = self.source_snapshot();
        self.dirty = false;
    }

    // ── stable identity for a sequence item ───────────────────────────────

    /// A stable identity for item `index` of the sequence at `seq_path`, or
    /// `None` when nothing can name it.
    ///
    /// The backend first ([`Backend::item_key`]) — it is the component that may
    /// have a real identity to hand — and otherwise the projection's own guess:
    /// a mapping item is named by whichever of its fields best names it on a
    /// page ([`page::title_keys`]/[`page::title_of`], the same answer the row
    /// already shows), and a scalar item by its own text. Neither is guaranteed
    /// unique, which is why every use of this treats a repeat as "the first
    /// one": a list of five identical strings has nothing to tell its items
    /// apart with, and behaving as though it did would be worse than falling
    /// back to the index.
    ///
    /// Public because a host holding an id of its own — a navigation stack, a
    /// breadcrumb — needs the same answer the model re-resolves against.
    pub fn item_key(&self, seq_path: &[Seg], index: usize) -> Option<String> {
        if let Ok(Some(key)) = self.backend.item_key(seq_path, index) {
            return Some(key);
        }
        let Some(Value::Seq(items)) = self.value_at(seq_path) else {
            return None;
        };
        let item = items.get(index)?;
        match item {
            Value::Map(_) => page::title_of(&page::title_keys(items), item),
            Value::Seq(_) => None,
            scalar => Some(tree::edit_seed(scalar)),
        }
    }

    /// The identity of every indexed step of `path`, taken against the tree as
    /// it stands — `None` at a step that is a key, or an item nothing names.
    fn keys_along(&self, path: &[Seg]) -> Vec<Option<String>> {
        path.iter()
            .enumerate()
            .map(|(i, seg)| match seg {
                Seg::Index(index) => self.item_key(&path[..i], *index),
                Seg::Key(_) => None,
            })
            .collect()
    }

    /// `path`, with every indexed step moved to wherever the item it named has
    /// ended up.
    ///
    /// A step whose item is gone, or that nothing could name, keeps its index
    /// and is left to the clamping the rebuild already does: a page whose
    /// container was deleted pops to the nearest surviving ancestor, which is
    /// the behaviour that was there before identity was.
    fn resolve_against(&self, path: &[Seg], keys: &[Option<String>]) -> Vec<Seg> {
        let mut out: Vec<Seg> = Vec::with_capacity(path.len());
        for (i, seg) in path.iter().enumerate() {
            match (seg, keys.get(i).and_then(Option::as_ref)) {
                (Seg::Index(index), Some(want)) => {
                    let len = self.seq_len(&out);
                    let found = (0..len).find(|j| self.item_key(&out, *j).as_ref() == Some(want));
                    out.push(Seg::Index(found.unwrap_or(*index)));
                }
                _ => out.push(seg.clone()),
            }
        }
        out
    }

    /// Where the page view is standing, by identity rather than by index — what
    /// an edit elsewhere in the document must not be allowed to re-point.
    ///
    /// Taken *before* an edit is applied, because an identity is read off the
    /// tree the path was taken against, and restored after: see
    /// [`restore_identities`](Self::restore_identities).
    fn identities(&self) -> Identities {
        Identities {
            focus: self.keys_along(&self.focus),
            memory: self
                .page_memory
                .keys()
                .map(|path| (path.clone(), self.keys_along(path)))
                .collect(),
        }
    }

    /// Move [`focus`](Self::focus) and the remembered cursors onto whatever the
    /// items they named have become.
    ///
    /// Run against the *new* tree, so every `item_key` call here reads the
    /// document as the edit left it. A reorder moves a path; a delete of an
    /// earlier sibling shifts it down; an append leaves it alone — and none of
    /// the three is a special case, because all three are "where did the thing
    /// I was looking at go".
    fn restore_identities(&mut self, ids: Identities) {
        self.focus = self.resolve_against(&self.focus.clone(), &ids.focus);
        let memory = std::mem::take(&mut self.page_memory);
        self.page_memory = ids
            .memory
            .into_iter()
            .filter_map(|(path, keys)| {
                let selected = memory.get(&path)?;
                Some((self.resolve_against(&path, &keys), *selected))
            })
            .collect();
    }

    // ── view derivation ───────────────────────────────────────────────────────

    /// Re-derive `value` + `rows` from the backend's current tree.
    fn reload(&mut self) -> Result<()> {
        self.reload_keeping(None)
    }

    /// [`reload`](Self::reload), re-pointing the page view's paths at the items
    /// they named before the edit when `ids` says what those were.
    ///
    /// Between reading the tree and rebuilding the pages, because the focus a
    /// page is built from must already be the corrected one — rebuilding twice
    /// would draw one frame of the wrong page.
    fn reload_keeping(&mut self, ids: Option<Identities>) -> Result<()> {
        self.value = self
            .backend
            .to_value()
            .map_err(|e| anyhow::anyhow!("reading value tree: {e}"))?;
        if let Some(ids) = ids {
            self.restore_identities(ids);
        }
        self.rebuild_rows();
        self.rebuild_pages();
        Ok(())
    }

    fn rebuild_rows(&mut self) {
        self.rows = tree::build_rows(&self.value, &self.collapsed, &self.hidden);
        for row in &mut self.rows {
            row.annotation = annotate::exactly_at(&self.annotations, &row.path).cloned();
        }
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
        self.root_page = self.page_at(&[]);
        self.page = if self.focus.is_empty() {
            self.root_page.clone()
        } else {
            self.page_at(&self.focus.clone())
        };
        self.parent_page = if self.focus.is_empty() {
            Page::default()
        } else {
            // The pane you came out of is the pane you *actually* came out of.
            //
            // One level out is the wrong answer once a row can compress: opening
            // `exports › journal` skips the `exports` page precisely because it
            // holds nothing but that one row, and drawing it on the left would
            // spend half a wide layout on the page the compression existed to
            // spare you. So walk out past every level a row compressed past, and
            // stop at the page that actually lists the row that was tapped.
            let mut parent = &self.focus[..self.focus.len() - 1];
            while !parent.is_empty()
                && page::is_compressed_past(&self.value, parent, &self.hidden, self.inline_budget)
            {
                parent = &parent[..parent.len() - 1];
            }
            self.page_at(parent)
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

    /// The selected row of the tree projection — an index into
    /// [`rows`](Self::rows).
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Put the tree cursor on `index`, clamped to the row list.
    ///
    /// **Switches to the tree projection first**, and that is the point of the
    /// method rather than a side effect. A row index is a coordinate in the row
    /// list a caller last rendered; it names nothing on a page. A host driving
    /// both surfaces — a metadata pane beside a settings page — would otherwise
    /// hand a row index to a model still standing in the page projection, where
    /// the very next [`delete_selected`](Self::delete_selected) reads the *page*
    /// cursor and quietly removes a different node.
    ///
    /// So the vocabularies assert. Every method that establishes a cursor names
    /// the projection its coordinates belong to ([`page_enter`](Self::page_enter)
    /// and the rest do the same for pages), and the methods that merely *read* a
    /// cursor stay neutral — a delete deletes what is selected, in whichever view
    /// the user is actually looking at.
    ///
    /// A no-op when the model is already in the tree.
    pub fn select_row(&mut self, index: usize) {
        self.set_view(ViewMode::Tree);
        self.selected = if self.rows.is_empty() {
            0
        } else {
            index.min(self.rows.len() - 1)
        };
    }

    pub fn move_down(&mut self) {
        // Tree vocabulary: assert the projection these coordinates belong to.
        self.set_view(ViewMode::Tree);
        if self.selected + 1 < self.rows.len() {
            self.selected += 1;
        }
    }

    pub fn move_up(&mut self) {
        // Tree vocabulary: assert the projection these coordinates belong to.
        self.set_view(ViewMode::Tree);
        self.selected = self.selected.saturating_sub(1);
    }

    /// `l`: expand a collapsed container, else step into its first child.
    pub fn expand_or_enter(&mut self) {
        // Tree vocabulary: assert the projection these coordinates belong to.
        self.set_view(ViewMode::Tree);
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
        // Tree vocabulary: assert the projection these coordinates belong to.
        self.set_view(ViewMode::Tree);
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
    /// sequence of scalars, anything a generous budget has poured onto one page
    /// — has no navigation to put in a sidebar, and splitting the width for it
    /// would cost half the room and buy nothing. A frontend checks this to fall
    /// back to a single full-width pane.
    ///
    /// The second case is the same waste one level along: when the page the
    /// cursor is on is the one that leads the split
    /// ([`page_leads_the_split`](Self::page_leads_the_split)), the right pane is
    /// a preview of what the cursor would open, and a page with nothing to open
    /// has no preview to put there.
    pub fn pages_would_degenerate(&self) -> bool {
        if !self.root_page.has_drills() {
            return true;
        }
        self.page_leads_the_split() && !self.page.has_drills()
    }

    /// Whether the page the cursor is on belongs in the *left* pane, with the
    /// right one previewing what the cursor would open.
    ///
    /// Two panes are two consecutive levels of one lineage, and the left one is
    /// the outermost that offers a choice. Usually that is the page the current
    /// one was opened from. But a page can be opened from one that has a single
    /// row on it — the root of `{repo: [...]}`, or any level
    /// [`enter_document`](Self::enter_document) started past — and drawing that
    /// on the left spends half the width on a row nobody can choose between.
    /// Then this page leads instead, and the pane that would have repeated its
    /// parent previews its child.
    pub fn page_leads_the_split(&self) -> bool {
        self.focus.is_empty() || !self.parent_page.has_choice()
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
        // Page vocabulary, like the rest. Re-entrant from `set_view`, which calls
        // this to carry the cursor across — but by then `view` is already
        // `Pages`, so the call below returns immediately rather than recursing.
        self.set_view(ViewMode::Pages);
        if tree::value_at(&self.value, path).is_none() {
            return;
        }
        let mut focus: Vec<Seg> = Vec::new();
        while focus.len() < path.len()
            && page::build_page(
                &self.value,
                &focus,
                &self.hidden,
                &self.demoted,
                self.inline_budget,
            )
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
        let mut page = page::build_page(
            &self.value,
            path,
            &self.hidden,
            &self.demoted,
            self.inline_budget,
        );
        self.annotate_comments(&mut page);
        page
    }

    /// Fill each item's comments from the backend.
    ///
    /// A pass after the build rather than a parameter to it: the page projection
    /// is a pure function of the value tree, and the value tree has no comments —
    /// they live in the editor's source, one read per node. Keeping the reads
    /// here leaves [`page::build_page`] callable on a bare `Value` (its tests,
    /// an embedder without a backend) and puts the only code that knows comments
    /// come from the *backend* next to the only code that has one.
    ///
    /// A read that fails leaves the item's comment `None`: a comment is
    /// decoration on a page, and a page that cannot show one is still the page.
    fn annotate_comments(&self, page: &mut Page) {
        for item in &mut page.items {
            item.leading_comment = self.backend.leading_comment(&item.path).ok().flatten();
            item.trailing_comment = self.backend.trailing_comment(&item.path).ok().flatten();
            // The host's findings ride the same pass, and for the same reason:
            // neither is in the value tree `build_page` is a function of.
            item.annotation = annotate::exactly_at(&self.annotations, &item.path).cloned();
        }
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
        // The page it would *open*, which for a compressed row is the far end of
        // the chain — previewing the single-row page in between would put the
        // pane's whole purpose (showing what you are about to open) to work
        // showing the name you are pointing at.
        Some(self.page_at(&item.descend_to))
    }

    /// `j` in the page view.
    pub fn page_move_down(&mut self) {
        // Page vocabulary: assert the projection this cursor belongs to.
        self.set_view(ViewMode::Pages);
        if self.page_selected + 1 < self.page.items.len() {
            self.page_selected += 1;
        }
    }

    /// `k` in the page view.
    pub fn page_move_up(&mut self) {
        // Page vocabulary: assert the projection this cursor belongs to.
        self.set_view(ViewMode::Pages);
        self.page_selected = self.page_selected.saturating_sub(1);
    }

    /// Stand on row `index` of the page — a click, where `j`/`k` are a walk.
    ///
    /// The page counterpart to [`select_row`](Self::select_row), and clamped
    /// the same way: a row past the end is the last row, and an empty page
    /// keeps the cursor at zero. It cannot land the cursor anywhere the walk
    /// could not, only faster.
    pub fn page_select(&mut self, index: usize) {
        // Page vocabulary: assert the projection this cursor belongs to.
        self.set_view(ViewMode::Pages);
        self.page_selected = index.min(self.page.items.len().saturating_sub(1));
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
        // Page vocabulary: assert the projection this cursor belongs to.
        self.set_view(ViewMode::Pages);
        let Some(item) = self.page_item() else {
            return;
        };
        if item.is_scalar() {
            // The picker where there is one, the text field where there is not
            // — see `begin_choose`, which is the fallback rather than a second
            // key to bind.
            self.begin_choose();
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
        // `descend_to`, not `path`: a compressed row names a chain of containers
        // that hold only each other, and opening it lands on the far end — the
        // first page with more on it than the name you just tapped. They are the
        // same path for every other row.
        let target = item.descend_to.clone();
        self.page_memory
            .insert(self.focus.clone(), self.page_selected);
        self.focus = target;
        self.page_selected = 0;
        self.rebuild_pages();
    }

    /// `h`/`Esc` in the page view: pop back to the page that *listed* the row you
    /// opened, restoring the cursor to it.
    ///
    /// One level out is the wrong answer once a row can compress, for the same
    /// reason it is the wrong left pane
    /// ([`rebuild_pages`](Self::rebuild_pages)): opening `exports › journal`
    /// deliberately skips the `exports` page because it holds nothing but that
    /// one row, and handing it back on the way out makes leaving cost two steps
    /// where arriving cost one — on a page whose only row is the name of the
    /// place you just left. So this walks out past every level a row compressed
    /// past, and lands where the row was tapped.
    ///
    /// Nothing becomes unreachable by it. A compressed row's
    /// [`path`](PageItem::path) is the outermost container, so renaming,
    /// deleting, reordering and adding to `exports` are all still that row's ops
    /// on the page this lands on — the skipped page never held anything else.
    pub fn page_back(&mut self) {
        // Page vocabulary: assert the projection this cursor belongs to.
        self.set_view(ViewMode::Pages);
        if self.focus.is_empty() {
            self.status = "already at the top".to_string();
            return;
        }
        let child = std::mem::take(&mut self.focus);
        let mut parent = &child[..child.len() - 1];
        while !parent.is_empty()
            && page::is_compressed_past(&self.value, parent, &self.hidden, self.inline_budget)
        {
            parent = &parent[..parent.len() - 1];
        }
        self.focus = parent.to_vec();
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
            self.begin_choose();
        }
    }

    // ── choosing ──────────────────────────────────────────────────────────

    /// The values a picker at `path` should offer, or `None` when there is no
    /// list to offer and a value must be typed.
    ///
    /// Two sources, in order. A [`Constraint::Enum`](crate::Constraint::Enum)
    /// rule answers from its own terms — retired ones included, flagged in the
    /// choice's `detail`, because a term already written in the document has to
    /// stay re-choosable. Otherwise the backend is asked
    /// ([`Backend::candidates`]), which is where a reference field's
    /// candidates come from; a backend over a standalone file has none.
    ///
    /// **A list's append position answers too.** Asked about a sequence, this
    /// re-asks about its first item (`path.0`) — a rule written with
    /// [`PathPat::each_item_of`](fig_schema::PathPat::each_item_of) is
    /// index-agnostic, so the placeholder matches whatever governs the items,
    /// which is the same trick the commit funnel's validation uses. A frontend
    /// offering "add to this list" therefore gets the list's vocabulary without
    /// having to guess an index that does not exist yet.
    pub fn choices_at(&self, path: &[Seg]) -> Option<Vec<Choice>> {
        if let Some((terms, _)) = self.rule_at(path).and_then(|r| r.enum_constraint()) {
            let choices = schema::choices_of(terms);
            if !choices.is_empty() {
                return Some(choices);
            }
        }
        if let Ok(Some(candidates)) = self.backend.candidates(path)
            && !candidates.is_empty()
        {
            return Some(candidates);
        }
        // A sequence has no vocabulary of its own; its *items* may.
        if matches!(self.value_at(path), Some(Value::Seq(_))) {
            let mut item = path.to_vec();
            item.push(Seg::Index(0));
            return self.choices_at(&item);
        }
        None
    }

    /// Open the picker on the selected node, or fall back to
    /// [`begin_edit`](Self::begin_edit) when there is nothing to pick from.
    ///
    /// The fallback is the point: a host binds *one* key and gets a list where
    /// there is a list and a text field where there is not, rather than having
    /// to ask first and bind two. A closed vocabulary makes the picker the only
    /// route to a value that validates, and free text stays reachable anyway
    /// ([`begin_edit`](Self::begin_edit)) — what is typed goes through the same
    /// validation the picker's values would.
    pub fn begin_choose(&mut self) {
        let Some(path) = self.selected_path() else {
            return;
        };
        let Some(choices) = self.choices_at(&path) else {
            self.begin_edit();
            return;
        };
        if self.value_at(&path).is_some_and(page::is_container) {
            // A container with an item vocabulary is a list to add to, not a
            // value to replace — an affordance that is not built yet.
            self.begin_edit();
            return;
        }
        self.mode = Mode::Choosing {
            path,
            choices,
            selected: 0,
            filter: String::new(),
        };
    }

    /// The choices the picker is currently showing — everything offered, cut to
    /// what the filter matches. Empty outside [`Mode::Choosing`].
    pub fn visible_choices(&self) -> Vec<&Choice> {
        match &self.mode {
            Mode::Choosing {
                choices, filter, ..
            } => choices.iter().filter(|c| c.matches(filter)).collect(),
            _ => Vec::new(),
        }
    }

    /// The choice under the picker's cursor, if any.
    pub fn choice_selected(&self) -> Option<&Choice> {
        match &self.mode {
            Mode::Choosing { selected, .. } => self.visible_choices().get(*selected).copied(),
            _ => None,
        }
    }

    /// Move the picker's cursor down one filtered row.
    pub fn choose_next(&mut self) {
        let len = self.visible_choices().len();
        if let Mode::Choosing { selected, .. } = &mut self.mode
            && *selected + 1 < len
        {
            *selected += 1;
        }
    }

    /// Move the picker's cursor up one filtered row.
    pub fn choose_prev(&mut self) {
        if let Mode::Choosing { selected, .. } = &mut self.mode {
            *selected = selected.saturating_sub(1);
        }
    }

    /// Narrow the picker by one more typed character (case-insensitive
    /// substring of the label). The cursor returns to the top of what is left,
    /// so what is highlighted is always a row that is on screen.
    pub fn choose_push(&mut self, c: char) {
        if let Mode::Choosing {
            filter, selected, ..
        } = &mut self.mode
        {
            filter.push(c);
            *selected = 0;
        }
    }

    /// Undo one character of the picker's filter.
    pub fn choose_backspace(&mut self) {
        if let Mode::Choosing {
            filter, selected, ..
        } = &mut self.mode
        {
            filter.pop();
            *selected = 0;
        }
    }

    /// Commit the choice under the cursor, replacing the node's value.
    ///
    /// A no-op (beyond leaving the picker) when the filter has cut the list to
    /// nothing: there is no value to write, and inventing one from what was
    /// typed would be the free-text path wearing the picker's clothes.
    pub fn choose_commit(&mut self) {
        let chosen = self.choice_selected().map(|c| c.value.clone());
        let Mode::Choosing { path, .. } = &self.mode else {
            return;
        };
        let path = path.clone();
        self.mode = Mode::Normal;
        match chosen {
            Some(value) => self.set_value_at(&path, value),
            None => self.status = "nothing matches".to_string(),
        }
    }

    /// Leave the picker, writing nothing.
    pub fn choose_cancel(&mut self) {
        self.mode = Mode::Normal;
        self.status = "choice cancelled".to_string();
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
        self.mode = Mode::Editing {
            buffer: seed,
            path,
            slot: EditSlot::Value,
        };
    }

    /// Open the selected node's leading comment block for editing, seeded with
    /// what is there (empty when there is none). Any node, container or scalar:
    /// a comment above a table is as much the table's as one above a key.
    pub fn begin_edit_leading_comment(&mut self) {
        self.begin_edit_comment(EditSlot::LeadingComment);
    }

    /// Open the selected node's trailing comment for editing, seeded with what is
    /// there. See [`begin_edit_leading_comment`](Self::begin_edit_leading_comment).
    pub fn begin_edit_trailing_comment(&mut self) {
        self.begin_edit_comment(EditSlot::TrailingComment);
    }

    fn begin_edit_comment(&mut self, slot: EditSlot) {
        let Some(path) = self.selected_path() else {
            return;
        };
        let read = match slot {
            EditSlot::LeadingComment => self.backend.leading_comment(&path),
            EditSlot::TrailingComment => self.backend.trailing_comment(&path),
            EditSlot::Value => unreachable!("begin_edit_comment is only called for a comment slot"),
        };
        let seed = match read {
            Ok(text) => text.unwrap_or_default(),
            Err(e) => {
                self.status = format!("rejected: {e}");
                return;
            }
        };
        self.mode = Mode::Editing {
            buffer: seed,
            path,
            slot,
        };
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
        let Mode::Editing { buffer, path, slot } = &mut self.mode else {
            return;
        };
        let buffer = std::mem::take(buffer);
        let path = std::mem::take(path);
        let slot = *slot;
        self.mode = Mode::Normal;

        match slot {
            EditSlot::Value => {
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
            // An empty buffer is "no comment", not "a comment saying nothing":
            // the one thing a user can type to mean *remove it*.
            EditSlot::LeadingComment => {
                let text = (!buffer.is_empty()).then_some(buffer.as_str());
                self.set_leading_comment(&path, text)
            }
            EditSlot::TrailingComment => {
                let text = (!buffer.is_empty()).then_some(buffer.as_str());
                self.set_trailing_comment(&path, text)
            }
        }
    }

    /// Set (or, with `None`, remove) the own-line comment block above the node at
    /// `path`, refreshing the view. The by-path counterpart of committing an
    /// [`EditSlot::LeadingComment`] edit, for an embedder or FFI.
    pub fn set_leading_comment(&mut self, path: &[Seg], text: Option<&str>) {
        self.commit(
            EditOp::SetLeadingComment {
                path: path.to_vec(),
                text: text.map(str::to_string),
            },
            path.to_vec(),
            if text.is_some() {
                "comment updated"
            } else {
                "comment removed"
            },
        );
    }

    /// Set (or, with `None`, remove) the same-line comment after the value at
    /// `path`, refreshing the view. See
    /// [`set_leading_comment`](Self::set_leading_comment).
    pub fn set_trailing_comment(&mut self, path: &[Seg], text: Option<&str>) {
        self.commit(
            EditOp::SetTrailingComment {
                path: path.to_vec(),
                text: text.map(str::to_string),
            },
            path.to_vec(),
            if text.is_some() {
                "comment updated"
            } else {
                "comment removed"
            },
        );
    }

    /// The own-line comment block above the node at `path`, if the backend
    /// reports one — a fresh read, not the copy on a page item.
    pub fn leading_comment_at(&self, path: &[Seg]) -> Option<String> {
        self.backend.leading_comment(path).ok().flatten()
    }

    /// The same-line comment after the value at `path`, if the backend reports
    /// one.
    pub fn trailing_comment_at(&self, path: &[Seg]) -> Option<String> {
        self.backend.trailing_comment(path).ok().flatten()
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
        // Derived *before* the apply, from the tree as it still stands: an
        // inverse is a statement about the document the op is addressed
        // against, and after the splice that document is gone.
        let inverse = self.invert(&op);
        // Taken before the splice: an item's identity is read off the tree the
        // path was taken against.
        let ids = self.identities();
        match self.backend.apply(op.clone()) {
            Ok(()) => {
                match inverse {
                    Some(inverse) => self.undo_stack.push(Change {
                        forward: op,
                        inverse,
                        anchor: anchor.clone(),
                    }),
                    // An op we cannot invert (a path that stopped resolving
                    // between the two reads, a rename of something that is not
                    // a key) is a hole in the history rather than a step in it,
                    // and a stack with a hole in the middle undoes to a document
                    // nobody ever had. Dropping what is behind it is the honest
                    // answer, and `history_len` says so.
                    None => self.undo_stack.clear(),
                }
                // A fresh edit is a new branch: what was undone is no longer
                // ahead of us.
                self.redo_stack.clear();
                self.edit_seq += 1;
                self.after_edit(&anchor, msg, ids);
                // A soft-warn overrides the success status so the user sees it.
                if let Some(why) = warn {
                    self.status = why.to_string();
                }
            }
            // The backend rolled back / declined; the document is untouched.
            Err(e) => self.status = format!("rejected: {e}"),
        }
    }

    // ── history ───────────────────────────────────────────────────────────

    /// How many edits are on the undo journal — the depth
    /// [`undo`](Self::undo) can walk back through.
    ///
    /// A host composing flower with another editor reads it to know whether
    /// there is anything of flower's to undo before it dispatches the keystroke
    /// (provui's session, holding flower's metadata beside leaf's body).
    pub fn history_len(&self) -> usize {
        self.undo_stack.len()
    }

    /// How many undone edits are available to [`redo`](Self::redo). Reset to 0
    /// by the next fresh commit.
    pub fn redo_len(&self) -> usize {
        self.redo_stack.len()
    }

    /// A number that increases on every successful commit, undo and redo, and
    /// on nothing else.
    ///
    /// Not a depth — undoing advances it as surely as editing does, because
    /// what it counts is *how many times the document has changed*, not how far
    /// from the start it is. A host holding two editors keeps one ordered
    /// history by recording which editor's sequence number moved, so "body
    /// edit, metadata edit, body edit" undoes in that order without either
    /// editor knowing the other exists.
    pub fn edit_seq(&self) -> u64 {
        self.edit_seq
    }

    /// Undo the most recent edit, putting the cursor back where it was made.
    ///
    /// The inverse goes through the same [`Backend::apply`] the edit did, so a
    /// backend that declines an edit declines its undo too — and a
    /// workspace-maintained key refuses here exactly as it refuses there. The
    /// schema is *not* re-consulted: the value being restored is one the
    /// document already held, and a vocabulary that has since tightened is not
    /// a reason to strand a user one edit away from where they were.
    ///
    /// Returns whether the document moved: `false` when there was nothing to
    /// undo, or the inverse was refused, and in either case with a status
    /// saying which. A host composing two editors dispatches an undo to one of
    /// them and needs to know whether it landed without snapshotting
    /// [`edit_seq`](Self::edit_seq) around the call.
    pub fn undo(&mut self) -> bool {
        let Some(change) = self.undo_stack.pop() else {
            self.status = "nothing to undo".to_string();
            return false;
        };
        if let Some(key) = self.managed_key_of(&change.inverse) {
            self.status = format!("rejected: `{key}` is maintained by the workspace");
            self.undo_stack.push(change);
            return false;
        }
        let ids = self.identities();
        match self.apply_all(&change.inverse) {
            Ok(()) => {
                self.edit_seq += 1;
                let anchor = change.anchor.clone();
                self.redo_stack.push(change);
                self.after_edit(&anchor, "undone", ids);
                self.reveal(&anchor);
                true
            }
            Err(e) => {
                self.status = format!("rejected: {e}");
                self.undo_stack.push(change);
                false
            }
        }
    }

    /// Redo the most recently undone edit. Cleared — and so a no-op — once a
    /// fresh edit has been committed on top. Returns whether the document
    /// moved, as [`undo`](Self::undo) does.
    pub fn redo(&mut self) -> bool {
        let Some(change) = self.redo_stack.pop() else {
            self.status = "nothing to redo".to_string();
            return false;
        };
        if let Some(key) = self.managed_key_of(std::slice::from_ref(&change.forward)) {
            self.status = format!("rejected: `{key}` is maintained by the workspace");
            self.redo_stack.push(change);
            return false;
        }
        let ids = self.identities();
        match self.apply_all(std::slice::from_ref(&change.forward)) {
            Ok(()) => {
                self.edit_seq += 1;
                let anchor = change.anchor.clone();
                self.undo_stack.push(change);
                self.after_edit(&anchor, "redone", ids);
                self.reveal(&anchor);
                true
            }
            Err(e) => {
                self.status = format!("rejected: {e}");
                self.redo_stack.push(change);
                false
            }
        }
    }

    /// The workspace-maintained top-level key one of `ops` would touch, if any
    /// — the same refusal [`commit`](Self::commit) makes, asked of a whole
    /// inverse at once so that nothing is half-applied before it fires.
    fn managed_key_of(&self, ops: &[EditOp]) -> Option<String> {
        ops.iter()
            .filter_map(op_root_key)
            .find(|k| self.derived.contains(*k))
            .map(str::to_string)
    }

    /// Apply every op in order. Each is atomic on its own
    /// ([`Backend::apply`]); the *sequence* is not, so a failure partway
    /// through — which needs a backend refusing an op it accepted the inverse
    /// of — leaves what ran in place and reports.
    fn apply_all(&mut self, ops: &[EditOp]) -> Result<(), crate::backend::BackendError> {
        for op in ops {
            self.backend.apply(op.clone())?;
        }
        Ok(())
    }

    /// Bring the node at `anchor` under the cursor, opening whatever stands
    /// between the cursor and it.
    ///
    /// [`after_edit`](Self::after_edit) re-anchors the selection, which is
    /// enough while the edit and the cursor are on the same page — they are,
    /// for an edit the cursor just made. An undo is the case where they are
    /// not: it changes a row the reader may have navigated away from several
    /// pages ago, and a status line saying "undone" over an unchanged screen
    /// is the one thing an undo must not be. So this *navigates*, in whichever
    /// projection is live.
    fn reveal(&mut self, anchor: &[Seg]) {
        if self.value_at(anchor).is_none() {
            return;
        }
        match self.view {
            // Already on screen, in either of the two ways it can be: the page
            // lists it (`after_edit` has just put the cursor on it), or it is
            // the container the cursor is standing *inside*. Navigating in
            // either case would take a reader out of the page they were on to
            // show them a row they can already see.
            ViewMode::Pages
                if self.focus.starts_with(anchor) || self.page.position_of(anchor).is_some() => {}
            ViewMode::Pages => self.focus_on(anchor),
            ViewMode::Tree => {
                for i in 0..anchor.len() {
                    self.collapsed.remove(&anchor[..i]);
                }
                self.rebuild_rows();
                self.select_path(anchor);
            }
        }
    }

    /// The ops that put the document back the way it is *now*, were `op` to be
    /// applied to it. `None` when the current tree cannot answer — an
    /// unresolvable path, a rename of something that is not a key.
    ///
    /// Read against the pre-edit tree by every arm, which is what makes the
    /// derivation total in one place instead of scattered through the ops.
    fn invert(&self, op: &EditOp) -> Option<Vec<EditOp>> {
        Some(match op {
            EditOp::ReplaceValue { path, .. } => vec![EditOp::ReplaceValue {
                path: path.clone(),
                value: self.value_at(path)?.clone(),
            }],
            EditOp::DeleteKey { path } => {
                let Some(Seg::Key(key)) = path.last() else {
                    return None;
                };
                let map_path = path[..path.len() - 1].to_vec();
                let value = self.value_at(path)?.clone();
                let keys = tree::map_keys(&self.value, &map_path)?;
                let mut ops = vec![EditOp::InsertKey {
                    map_path: map_path.clone(),
                    key: key.clone(),
                    value,
                }];
                // The entry comes back at the end of the mapping, so its
                // comments are addressed there and the reorder carries them to
                // its old position with it.
                let mut at = map_path.clone();
                at.push(Seg::Key(key.clone()));
                ops.extend(self.comment_restores(path, &at));
                ops.push(EditOp::ReorderKeys { map_path, keys });
                ops
            }
            EditOp::RemoveItem { seq_path, index } => {
                let mut item_path = seq_path.clone();
                item_path.push(Seg::Index(*index));
                let value = self.value_at(&item_path)?.clone();
                let len = tree::seq_len(&self.value, seq_path)?;
                // After the removal the list is one shorter, so the append
                // lands at `len - 1` and the move takes it back to `index`.
                let landed = len.checked_sub(1)?;
                let mut landed_path = seq_path.clone();
                landed_path.push(Seg::Index(landed));
                let mut ops = vec![EditOp::AppendItem {
                    seq_path: seq_path.clone(),
                    value,
                }];
                ops.extend(self.comment_restores(&item_path, &landed_path));
                if landed != *index {
                    ops.push(EditOp::MoveItem {
                        seq_path: seq_path.clone(),
                        from: landed,
                        to: *index,
                    });
                }
                ops
            }
            EditOp::InsertKey { map_path, key, .. } => {
                let mut path = map_path.clone();
                path.push(Seg::Key(key.clone()));
                match self.value_at(&path) {
                    // An insert onto a key that is already there is an
                    // overwrite on an upserting backend (the case `EditOp`
                    // leaves unspecified), and inverts as one.
                    Some(old) => vec![EditOp::ReplaceValue {
                        path,
                        value: old.clone(),
                    }],
                    None => vec![EditOp::DeleteKey { path }],
                }
            }
            EditOp::AppendItem { seq_path, .. } => vec![EditOp::RemoveItem {
                seq_path: seq_path.clone(),
                index: tree::seq_len(&self.value, seq_path)?,
            }],
            EditOp::MoveItem { seq_path, from, to } => vec![EditOp::MoveItem {
                seq_path: seq_path.clone(),
                from: *to,
                to: *from,
            }],
            EditOp::ReorderKeys { map_path, .. } => vec![EditOp::ReorderKeys {
                map_path: map_path.clone(),
                keys: tree::map_keys(&self.value, map_path)?,
            }],
            EditOp::RenameKey { path, new_key } => {
                let Some(Seg::Key(old)) = path.last() else {
                    return None;
                };
                let mut renamed = path[..path.len() - 1].to_vec();
                renamed.push(Seg::Key(new_key.clone()));
                vec![EditOp::RenameKey {
                    path: renamed,
                    new_key: old.clone(),
                }]
            }
            EditOp::SetLeadingComment { path, .. } => vec![EditOp::SetLeadingComment {
                path: path.clone(),
                text: self.backend.leading_comment(path).ok().flatten(),
            }],
            EditOp::SetTrailingComment { path, .. } => vec![EditOp::SetTrailingComment {
                path: path.clone(),
                text: self.backend.trailing_comment(path).ok().flatten(),
            }],
        })
    }

    /// The comment ops that put `from`'s comments onto the node at `to` — how
    /// a deleted entry comes back with what was written above it.
    ///
    /// Only for comments that are actually there: a backend that reads none
    /// (or a format with no comment syntax) contributes nothing, rather than a
    /// pair of removals the undo would have to survive.
    fn comment_restores(&self, from: &[Seg], to: &[Seg]) -> Vec<EditOp> {
        let mut ops = Vec::new();
        if let Ok(Some(text)) = self.backend.leading_comment(from) {
            ops.push(EditOp::SetLeadingComment {
                path: to.to_vec(),
                text: Some(text),
            });
        }
        if let Ok(Some(text)) = self.backend.trailing_comment(from) {
            ops.push(EditOp::SetTrailingComment {
                path: to.to_vec(),
                text: Some(text),
            });
        }
        ops
    }

    /// Shared tail of a successful mutation: refresh the view, re-anchor
    /// selection, mark dirty, set the status line.
    fn after_edit(&mut self, anchor: &[Seg], msg: &str, ids: Identities) {
        if let Err(e) = self.reload_keeping(Some(ids)) {
            self.status = format!("view refresh failed: {e}");
            return;
        }
        self.select_path(anchor);
        // Derived from the bytes, not set: undoing back to what was saved is
        // a clean document, and no journal depth can say that for itself.
        self.dirty = self.source_snapshot() != self.saved_source;
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
        | EditOp::RenameKey { path, .. }
        | EditOp::SetLeadingComment { path, .. }
        | EditOp::SetTrailingComment { path, .. } => first_key(path),
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
    use crate::schema::Constraint;
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
    fn a_fresh_model_has_nothing_to_report() {
        // The status line carries refusals. A model that has refused nothing
        // has nothing for it, and a frontend reads the empty string as "draw no
        // bar" rather than having to know which openings words are noise.
        assert!(
            sample_model().status.is_empty(),
            "status: {}",
            sample_model().status
        );
    }

    #[test]
    fn page_items_carry_the_comments_written_on_them() {
        let model = sample_model();
        let page = model.root_page();
        let item = |k: &str| {
            page.items
                .iter()
                .find(|i| i.path == [Seg::Key(k.into())])
                .unwrap_or_else(|| panic!("no item {k}"))
        };
        assert_eq!(
            item("title").leading_comment.as_deref(),
            Some("flower sample config — comments and formatting below should survive edits")
        );
        assert_eq!(
            item("server").leading_comment.as_deref(),
            Some("the server block")
        );
        assert_eq!(item("version").leading_comment, None);
        assert_eq!(item("version").trailing_comment, None);
        // The projection over a bare `Value` knows nothing of them: it is the
        // model's pass that fills them, so a page built any other way has none.
        let bare = page::build_page(
            &model.value,
            &[],
            &HashSet::new(),
            &HashSet::new(),
            InlineBudget::default(),
        );
        assert!(bare.items.iter().all(|i| i.leading_comment.is_none()));
    }

    #[test]
    fn a_comment_is_edited_through_the_same_footer_as_a_value() {
        let mut model = sample_model();
        select(
            &mut model,
            &[Seg::Key("server".into()), Seg::Key("port".into())],
        );

        model.begin_edit_trailing_comment();
        assert!(matches!(
            model.mode,
            Mode::Editing {
                slot: EditSlot::TrailingComment,
                ..
            }
        ));
        type_value(&mut model, "dev only");
        let src = model.source_snapshot();
        assert!(src.contains("port = 8080 # dev only\n"), "{src}");
        assert!(model.dirty);
        assert_eq!(model.status, "comment updated");
        // …and the page shows it without a second read.
        let server = model.page_at(&[Seg::Key("server".into())]);
        let item = server
            .items
            .iter()
            .find(|i| i.path.last() == Some(&Seg::Key("port".into())))
            .expect("port is on server's page");
        assert_eq!(item.trailing_comment.as_deref(), Some("dev only"));

        // Reopening seeds the footer with what is there.
        model.begin_edit_trailing_comment();
        if let Mode::Editing { buffer, .. } = &model.mode {
            assert_eq!(buffer, "dev only");
        } else {
            panic!("not editing");
        }
        // An empty commit removes it.
        type_value(&mut model, "");
        assert!(model.source_snapshot().contains("port = 8080\n"));
        assert_eq!(model.status, "comment removed");

        // The block above, replaced whole — on a container as readily as a key.
        select(&mut model, &[Seg::Key("server".into())]);
        model.begin_edit_leading_comment();
        if let Mode::Editing { buffer, slot, .. } = &model.mode {
            assert_eq!(buffer, "the server block");
            assert_eq!(*slot, EditSlot::LeadingComment);
        } else {
            panic!("not editing");
        }
        type_value(&mut model, "where it listens");
        let src = model.source_snapshot();
        assert!(src.contains("# where it listens\n[server]"), "{src}");
        assert!(!src.contains("the server block"), "{src}");
        assert!(src.contains("port = 8080"), "value untouched");
    }

    #[test]
    fn a_flow_item_owns_no_leading_comment_and_the_parents_block_is_never_edited_through_it() {
        // An item of a one-line array sits on its parent's line and owns no
        // line to comment. fig (since core 2.9) reports none for it, deletes
        // nothing through it, and refuses to add one — so the page shows the
        // block once, on the container, and no write through an item can take
        // it. Before that fix the model guarded this itself, by comparing the
        // item's reported comment with its parent's; the guard is gone.
        let src = "\
# the members
members = [\"a\", \"b\"]
";
        let backend = FigBackend::open(src.as_bytes(), Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.fit_to_room(20);
        let page = model.root_page();
        let members = [Seg::Key("members".into())];
        let item0 = [Seg::Key("members".into()), Seg::Index(0)];
        let of = |p: &[Seg]| page.items.iter().find(|i| i.path == p).expect("item");
        assert_eq!(of(&members).leading_comment.as_deref(), Some("the members"));
        assert_eq!(of(&item0).leading_comment, None, "not repeated per item");
        assert_eq!(model.leading_comment_at(&item0), None);

        // Adding through the item is refused, and the source is untouched.
        model.set_leading_comment(&item0, Some("mine"));
        assert!(model.status.starts_with("rejected"), "{}", model.status);
        assert!(!model.dirty);
        assert_eq!(model.source_snapshot(), src, "parent's block kept");

        // Removing through the item removes nothing — there is nothing there.
        model.set_leading_comment(&item0, None);
        assert_eq!(model.source_snapshot(), src, "parent's block kept");

        // The container's own comment is still editable as its own.
        model.set_leading_comment(&members, Some("renamed"));
        assert!(model.source_snapshot().contains("# renamed\nmembers"));
    }

    #[test]
    fn comment_ops_by_path_refresh_the_page() {
        let mut model = sample_model();
        let host = [Seg::Key("server".into()), Seg::Key("host".into())];
        model.set_leading_comment(&host, Some("first\nsecond"));
        assert_eq!(
            model.leading_comment_at(&host).as_deref(),
            Some("first\nsecond")
        );
        let page = model.page_at(&[Seg::Key("server".into())]);
        let item = page.items.iter().find(|i| i.path == host).unwrap();
        assert_eq!(item.leading_comment.as_deref(), Some("first\nsecond"));
        model.set_leading_comment(&host, None);
        assert_eq!(model.leading_comment_at(&host), None);
        assert!(!model.source_snapshot().contains("first"));
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
        use fig_schema::{FieldType, PathPat, Term};
        let src = "audience = [\"public\"]\ntitle = \"note\"\n";
        let backend = FigBackend::open(src.as_bytes(), Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_schema(crate::schema::Schema::new(vec![
            FieldRule::new(PathPat::each_item_of("audience"))
                .ty(FieldType::Str)
                .constraint(Constraint::Enum {
                    values: vec![Term::value("public"), Term::value("private")],
                    closed: true,
                }),
        ]));

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
        use fig_schema::{FieldType, PathPat, Term};
        let src = "audience = [\"public\"]\ntitle = \"note\"\n";
        let backend = FigBackend::open(src.as_bytes(), Format::Toml).expect("open");
        let mut model =
            Model::with_hidden(backend, vec!["title".into(), "updated".into()]).expect("model");
        model.set_schema(crate::schema::Schema::new(vec![
            // Present in the document — already reachable, so never offered.
            FieldRule::new(PathPat::key("audience")).ty(FieldType::Str),
            // An each-item rule governs *within* a field; it names none.
            FieldRule::new(PathPat::each_item_of("audience"))
                .ty(FieldType::Str)
                .constraint(Constraint::Enum {
                    values: vec![Term::value("public")],
                    closed: true,
                }),
            // Declared, absent, not managed — the one to offer.
            FieldRule::new(PathPat::key("created")).ty(FieldType::Str),
            // Declared and absent, but the embedder manages it.
            FieldRule::new(PathPat::key("updated")).ty(FieldType::Str),
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
        use fig_schema::{FieldType, PathPat};
        let src = "code = \"x\"\n";
        let backend = FigBackend::open(src.as_bytes(), Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_schema(crate::schema::Schema::new(vec![
            FieldRule::new(PathPat::key("code")).ty(FieldType::Str),
        ]));

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
        use fig_schema::{FieldType, PathPat};
        let src = "tags = [\"alpha\"]\n\n[meta]\nk = \"v\"\n";
        let backend = FigBackend::open(src.as_bytes(), Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_schema(crate::schema::Schema::new(vec![
            // The *items* of `tags` are strings — the list itself is a seq.
            FieldRule::new(PathPat::each_item_of("tags")).ty(FieldType::Str),
            FieldRule::new(PathPat::key("year")).ty(FieldType::Str),
            FieldRule::new(PathPat(vec![
                fig_schema::SegPat::Key("meta".into()),
                fig_schema::SegPat::Key("code".into()),
            ]))
            .ty(FieldType::Str),
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
    fn raising_the_inline_budget_turns_the_root_page_into_the_document() {
        let mut model = paged_model();
        model.set_inline_budget(InlineBudget::new(99, 8));

        // Everything inlines, so the cursor can stand on the deepest member
        // without ever leaving the root page…
        model.focus_on(&[key("server"), key("limits"), key("timeout")]);
        assert!(model.focus().is_empty());
        assert_eq!(selected_label(&model), "timeout");
        assert!(model.page().items.iter().any(|i| i.inset == 2));

        // …and with nothing left to drill into, a second pane has no job.
        assert!(model.pages_would_degenerate());

        // Back to the default, the same node is reached through its page again.
        model.set_inline_budget(InlineBudget::default());
        model.focus_on(&[key("server"), key("limits"), key("timeout")]);
        assert_eq!(model.focus(), &[key("server")]);
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

    /// The `repos.figl` shape: one key, holding a list too long to inline.
    fn list_model() -> Model<FigBackend> {
        let items: Vec<String> = (0..22)
            .map(|i| format!(r#"{{"name": "r{i}", "lang": "rust"}}"#))
            .collect();
        let src = format!(r#"{{"repo": [{}]}}"#, items.join(", "));
        let backend = FigBackend::open(src.as_bytes(), Format::Json).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        model
    }

    #[test]
    fn a_document_that_is_one_list_opens_on_the_list() {
        let mut model = list_model();
        // Before: a root page whose one row names the file you just opened.
        assert_eq!(page_labels(&model), ["repo"]);

        model.enter_document();
        assert_eq!(model.focus(), &[Seg::Key("repo".into())]);
        assert_eq!(model.page().items.len(), 22);
        assert_eq!(model.page_selected(), 0);
        // The page it skipped is one step out, not gone: `repo` still renames,
        // deletes and takes an append there.
        model.page_back();
        assert_eq!(page_labels(&model), ["repo"]);
    }

    #[test]
    fn a_root_page_with_something_to_say_is_opened_where_it_is() {
        let mut model = paged_model();
        model.enter_document();
        assert!(model.focus().is_empty());
        assert_eq!(selected_label(&model), "title");
    }

    #[test]
    fn the_page_leads_the_split_when_the_one_behind_it_holds_a_single_row() {
        let mut model = list_model();
        model.enter_document();
        // The root page holds one row, so drawing it beside this one would
        // spend half the width on something nobody can choose between. This
        // page leads instead, and the other pane previews what it opens.
        assert!(model.page_leads_the_split());
        assert!(!model.pages_would_degenerate());
        assert_eq!(model.peek_page().expect("the first repo").items.len(), 2);

        // A root page with four rows on it is worth a pane, so it keeps one.
        let mut model = paged_model();
        model.focus_on(&[Seg::Key("server".into())]);
        model.page_enter();
        assert!(!model.page_leads_the_split());
    }

    #[test]
    fn fitting_the_room_puts_a_document_that_fits_on_one_page() {
        let mut model = paged_model();
        assert!(model.page().has_drills());

        // Twelve rows of document, and room for them.
        model.fit_to_room(12);
        assert!(!model.page().has_drills());
        assert!(model.pages_would_degenerate());
        assert_eq!(page_labels(&model).len(), 12);

        // One row short and the founding rule is back.
        model.fit_to_room(11);
        assert!(model.page().has_drills());
        assert_eq!(
            page_labels(&model),
            ["title", "version", "enabled", "server"]
        );
    }

    #[test]
    fn fitting_the_room_leaves_the_cursor_and_the_focus_where_they_were() {
        // A resize is not a navigation. It changes how much of the document a
        // page shows, and nothing about where the reader is in it.
        let mut model = list_model();
        model.enter_document();
        model.page_move_down();
        model.page_move_down();
        let (focus, at) = (model.focus().to_vec(), selected_label(&model));
        model.fit_to_room(80);
        assert_eq!(model.focus(), focus.as_slice());
        assert_eq!(selected_label(&model), at);
    }

    #[test]
    fn a_document_poured_onto_one_page_wastes_a_second_pane_wherever_you_are() {
        // Nothing to navigate to from the root, so there is no lineage to put
        // two panes on — even standing one level in, where a parent page and a
        // page would otherwise be two halves that repeat each other.
        let mut model = list_model();
        model.enter_document();
        model.set_inline_budget(InlineBudget::new(99, 8));
        assert!(!model.focus().is_empty());
        assert!(model.pages_would_degenerate());
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
    fn opening_a_compressed_row_lands_past_the_pages_that_say_nothing() {
        let backend = FigBackend::open(
            br#"{"exports": {"journal": {"label": "x", "gate": {"f": 1}}}, "z": 1}"#,
            Format::Json,
        )
        .expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);

        model.page_enter();
        // One step, two levels: the `exports` page held nothing but `journal`.
        assert_eq!(model.focus(), &[key("exports"), key("journal")]);
        assert_eq!(
            model.page().breadcrumb("‹document›"),
            "exports › journal",
            "the trail still shows what was skipped"
        );

        // Backing out retraces the step: one tap in was two levels, so one tap
        // out is two levels, and it lands on the page that listed the row rather
        // than on the page the compression existed to skip.
        model.page_back();
        assert!(model.focus().is_empty());
        // The cursor is on the row that was opened, which still addresses
        // `exports` and still renames it.
        assert_eq!(
            model.page_item().map(|i| i.label.clone()),
            Some("exports".into())
        );
        assert!(model.page_item().unwrap().can_rename());
    }

    #[test]
    fn the_left_pane_is_the_page_that_listed_the_row_not_the_level_above() {
        let backend = FigBackend::open(
            br#"{"exports": {"journal": {"label": "x", "gate": {"f": 1}}}, "z": 1}"#,
            Format::Json,
        )
        .expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        model.page_enter();
        assert_eq!(model.focus(), &[key("exports"), key("journal")]);

        // One level out is `exports`, whose page holds nothing but the row that
        // was tapped — the page the compression exists to skip. The left pane
        // walks past it to the page that actually listed the row.
        assert!(
            model.parent_page().focus.is_empty(),
            "the root, not `exports`"
        );
        // And it can still mark what was opened: the compressed row answers for
        // its whole chain.
        let marked = model
            .parent_page()
            .position_of(model.focus())
            .expect("marked");
        assert_eq!(model.parent_page().items[marked].label, "exports");

        // And backing out agrees with the pane: `exports` is skipped both ways,
        // so the page on the left is the page you land on.
        model.page_back();
        assert!(model.focus().is_empty());
        assert_eq!(model.focus(), model.parent_page().focus);
    }

    #[test]
    fn a_compressed_row_still_answers_ops_as_its_outermost_node() {
        let backend = FigBackend::open(
            br#"{"exports": {"journal": {"label": "x", "gate": {"f": 1}}}, "z": 1}"#,
            Format::Json,
        )
        .expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);

        // Deleting a row reading `exports › journal` takes the whole chain, so
        // no empty `exports: {}` is left behind to delete separately.
        model.delete_selected();
        assert!(!model.source_snapshot().contains("exports"));
        assert!(!model.source_snapshot().contains("journal"));
        assert!(model.source_snapshot().contains('z'));
    }

    /// A host driving both surfaces — a metadata pane beside a settings page —
    /// hands a *row* index to a model left standing in the page projection. The
    /// index means nothing there, and before `select_row` asserted the tree the
    /// delete that followed read the page cursor and removed a different node.
    #[test]
    fn a_row_index_deletes_the_row_it_names_even_from_the_page_projection() {
        let backend = FigBackend::open(
            br#"{"alpha": 1, "beta": 2, "gamma": {"inner": 3}}"#,
            Format::Json,
        )
        .expect("open");
        let mut model = Model::new(backend).expect("model");

        // Go and stand somewhere in the page projection, with its cursor on a
        // different node than the row index below names.
        model.set_view(ViewMode::Pages);
        model.page_move_down();
        assert_eq!(
            model.page_item().map(|i| i.label.clone()),
            Some("beta".into())
        );

        // Now the other surface speaks, in its own coordinates, without first
        // announcing a switch.
        model.select_row(0);
        model.delete_selected();

        assert!(
            !model.source_snapshot().contains("alpha"),
            "row 0 was `alpha`"
        );
        assert!(
            model.source_snapshot().contains("beta"),
            "the page cursor was not the target"
        );
    }

    /// The mirror: page vocabulary asserts pages, so a page op after tree work
    /// acts on the page cursor rather than on whatever row was last selected.
    #[test]
    fn a_page_op_acts_on_the_page_cursor_even_from_the_tree_projection() {
        // `gamma` holds a container *and* a scalar, so it neither inlines into
        // the root page nor compresses into a chain — it is a plain drill row.
        let backend = FigBackend::open(
            br#"{"alpha": 1, "beta": 2, "gamma": {"inner": {"deep": 3}, "flag": true}}"#,
            Format::Json,
        )
        .expect("open");
        let mut model = Model::new(backend).expect("model");

        model.select_row(0);
        assert_eq!(model.view(), ViewMode::Tree);

        // `page_enter` is page vocabulary; it must not be read against the tree.
        model.page_move_down();
        model.page_move_down();
        model.page_enter();
        assert_eq!(model.view(), ViewMode::Pages);
        assert_eq!(model.focus(), &[key("gamma")]);
    }

    #[test]
    fn backing_out_past_the_root_is_inert() {
        let mut model = paged_model();
        model.page_back();
        assert!(model.focus().is_empty());
        assert_eq!(model.page_selected(), 0);
    }

    /// Arriving and leaving cost the same number of steps.
    ///
    /// `views` holds only `date`, so its row compresses and `page_enter` lands
    /// straight on `views.date`. Popping one raw segment would put you on the
    /// `views` page — one row, named `date`, which is the page compression
    /// exists to skip — and make the way out twice as long as the way in.
    #[test]
    fn backing_out_retraces_what_entering_skipped() {
        let backend = FigBackend::open(
            br#"{"views": {"date": {"icon": "calendar", "group": ["created"], "by": "year"}}, "fixity": "all"}"#,
            Format::Json,
        )
        .expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);

        // One step in, past `views`, to the first page with more on it than the
        // name that was tapped.
        model.page_enter();
        assert_eq!(model.focus(), &[key("views"), key("date")]);

        // One step out, to the page that listed the row — not to `views`.
        model.page_back();
        assert!(model.focus().is_empty());
        // And the cursor is back on the row that was opened: a compressed row
        // answers for its whole chain, so the child path finds it.
        assert_eq!(model.page_selected(), 0);
    }

    /// The skipped page held nothing but the chain, so skipping it takes no
    /// operation away: the row on the page we land on still addresses `views`.
    #[test]
    fn the_skipped_level_is_still_operable_from_the_row() {
        let backend = FigBackend::open(
            br#"{"views": {"date": {"icon": "calendar", "group": ["created"], "by": "year"}}, "fixity": "all"}"#,
            Format::Json,
        )
        .expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);

        model.page_enter();
        model.page_back();
        let row = model.page_item().expect("a row under the cursor");
        assert_eq!(row.path, vec![key("views")]);
        assert_eq!(row.descend_to, vec![key("views"), key("date")]);
    }

    #[test]
    fn the_two_panes_are_consecutive_levels_of_one_lineage() {
        // Every level here holds two things, so no row compresses and each
        // `page_enter` moves exactly one level — which is what this is about.
        let backend = FigBackend::open(
            br#"{"jobs": {"plan": {"steps": {"a": 1, "b": {"c": 2}}, "id": 3}, "name": "x"}}"#,
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

    // ── stable identity for a sequence item ───────────────────────────────

    /// Five items, each named by a field that tells it from the others — the
    /// shape a page of a list actually has.
    fn steps_model() -> Model<FigBackend> {
        let src = concat!(
            r#"{"steps": ["#,
            r#"{"name": "alpha", "run": "a"},"#,
            r#"{"name": "bravo", "run": "b"},"#,
            r#"{"name": "charlie", "run": "c"},"#,
            r#"{"name": "delta", "run": "d"},"#,
            r#"{"name": "echo", "run": "e"}"#,
            r#"], "tags": ["x", "y", "z"]}"#,
        );
        let backend = FigBackend::open(src.as_bytes(), Format::Json).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        // Nothing inlines, so each step is a page you can stand *in* — which is
        // the case a reorder re-points and the one this is about.
        model.set_inline_budget(InlineBudget::new(0, 0));
        model
    }

    /// The name of the step the page is standing on, read out of the document.
    fn focused_name(model: &Model<FigBackend>) -> Option<String> {
        let mut path = model.focus().to_vec();
        path.push(Seg::Key("name".into()));
        match model.value_at(&path) {
            Some(Value::Str(s)) => Some(s.clone()),
            _ => None,
        }
    }

    #[test]
    fn a_reorder_does_not_re_point_the_page_you_have_open() {
        let mut model = steps_model();
        let steps = vec![Seg::Key("steps".into())];
        let mut third = steps.clone();
        third.push(Seg::Index(2));
        model.focus_on(&third);
        model.page_enter();
        assert_eq!(focused_name(&model).as_deref(), Some("charlie"));

        // `alpha` goes to the end, so everything above it shifts down one.
        model.commit(
            EditOp::MoveItem {
                seq_path: steps.clone(),
                from: 0,
                to: 4,
            },
            steps.clone(),
            "moved",
        );
        assert_eq!(model.focus(), [Seg::Key("steps".into()), Seg::Index(1)]);
        assert_eq!(
            focused_name(&model).as_deref(),
            Some("charlie"),
            "the page is still the step it was opened on"
        );

        // …and the undo puts it back, by the same rule in reverse.
        model.undo();
        assert_eq!(model.focus(), [Seg::Key("steps".into()), Seg::Index(2)]);
        assert_eq!(focused_name(&model).as_deref(), Some("charlie"));
    }

    #[test]
    fn deleting_an_earlier_sibling_does_not_re_point_it_either() {
        let mut model = steps_model();
        let steps = vec![Seg::Key("steps".into())];
        let mut third = steps.clone();
        third.push(Seg::Index(2));
        model.focus_on(&third);
        model.page_enter();

        model.commit(
            EditOp::RemoveItem {
                seq_path: steps.clone(),
                index: 0,
            },
            steps.clone(),
            "deleted",
        );
        assert_eq!(model.focus(), [Seg::Key("steps".into()), Seg::Index(1)]);
        assert_eq!(focused_name(&model).as_deref(), Some("charlie"));

        // An append after it changes nothing, which is the third case and the
        // one that must not move.
        model.commit(
            EditOp::AppendItem {
                seq_path: steps.clone(),
                value: Value::Map(vec![(
                    Value::Str("name".into()),
                    Value::Str("foxtrot".into()),
                )]),
            },
            steps,
            "appended",
        );
        assert_eq!(model.focus(), [Seg::Key("steps".into()), Seg::Index(1)]);
        assert_eq!(focused_name(&model).as_deref(), Some("charlie"));
    }

    #[test]
    fn a_page_the_edit_removed_falls_back_to_the_clamping_it_always_had() {
        let mut model = steps_model();
        let steps = vec![Seg::Key("steps".into())];
        let mut third = steps.clone();
        third.push(Seg::Index(2));
        model.focus_on(&third);
        model.page_enter();

        model.commit(
            EditOp::RemoveItem {
                seq_path: steps.clone(),
                index: 2,
            },
            steps,
            "deleted",
        );
        // Nothing to re-find: the key `charlie` named is gone, so the index is
        // kept and the page is whatever is at that index now — the clamping
        // that was there before identity was. Identity buys back the cases
        // where the item still exists, and claims nothing about the one where
        // it does not.
        assert_eq!(model.focus(), [Seg::Key("steps".into()), Seg::Index(2)]);
        assert_eq!(focused_name(&model).as_deref(), Some("delta"));
    }

    #[test]
    fn a_scalar_sequence_is_identified_by_its_text() {
        let mut model = steps_model();
        let tags = vec![Seg::Key("tags".into())];
        assert_eq!(model.item_key(&tags, 0).as_deref(), Some("x"));
        assert_eq!(model.item_key(&tags, 2).as_deref(), Some("z"));

        model.commit(
            EditOp::MoveItem {
                seq_path: tags.clone(),
                from: 0,
                to: 2,
            },
            tags.clone(),
            "moved",
        );
        // The text went with the item, so the key follows it to its new index.
        assert_eq!(model.item_key(&tags, 2).as_deref(), Some("x"));
        assert_eq!(model.item_key(&tags, 0).as_deref(), Some("y"));

        // A mapping item takes the name the row already shows it by, and an
        // item nothing can name has no key at all.
        assert_eq!(
            model.item_key(&[Seg::Key("steps".into())], 3).as_deref(),
            Some("delta")
        );
        assert!(model.item_key(&[Seg::Key("nope".into())], 0).is_none());
    }

    #[test]
    fn a_backend_key_wins_over_the_inferred_one() {
        /// A backend that names an item by its link target, as one over a list
        /// of references would.
        struct WithKeys(FigBackend);
        impl Backend for WithKeys {
            fn apply(&mut self, op: EditOp) -> Result<(), crate::backend::BackendError> {
                self.0.apply(op)
            }
            fn to_value(&self) -> Result<Value, crate::backend::BackendError> {
                self.0.to_value()
            }
            fn source(&self) -> Result<String, crate::backend::BackendError> {
                self.0.source()
            }
            fn item_key(
                &self,
                seq_path: &[Seg],
                index: usize,
            ) -> Result<Option<String>, crate::backend::BackendError> {
                let mut path = seq_path.to_vec();
                path.push(Seg::Index(index));
                path.push(Seg::Key("id".into()));
                Ok(match tree::value_at(&self.to_value()?, &path) {
                    Some(Value::Str(s)) => Some(s.clone()),
                    _ => None,
                })
            }
        }

        let src = r#"{"links": [{"id": "a", "name": "one"}, {"id": "b", "name": "one"}]}"#;
        let backend = WithKeys(FigBackend::open(src.as_bytes(), Format::Json).expect("open"));
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        model.set_inline_budget(InlineBudget::new(0, 0));
        let links = vec![Seg::Key("links".into())];

        // Both items would infer the *same* title; the backend tells them apart.
        assert_eq!(model.item_key(&links, 0).as_deref(), Some("a"));
        assert_eq!(model.item_key(&links, 1).as_deref(), Some("b"));

        let mut second = links.clone();
        second.push(Seg::Index(1));
        model.focus_on(&second);
        model.page_enter();
        model.commit(
            EditOp::RemoveItem {
                seq_path: links,
                index: 0,
            },
            Vec::new(),
            "deleted",
        );
        assert_eq!(model.focus(), [Seg::Key("links".into()), Seg::Index(0)]);
        let mut id = model.focus().to_vec();
        id.push(Seg::Key("id".into()));
        assert_eq!(model.value_at(&id), Some(&Value::Str("b".into())));
    }

    // ── the picker ────────────────────────────────────────────────────────

    /// A backend that answers for a link field, the way a workspace-aware one
    /// would — the injection point `Backend::candidates` exists to be.
    struct WithCandidates(FigBackend);

    impl Backend for WithCandidates {
        fn apply(&mut self, op: EditOp) -> Result<(), crate::backend::BackendError> {
            self.0.apply(op)
        }
        fn to_value(&self) -> Result<Value, crate::backend::BackendError> {
            self.0.to_value()
        }
        fn source(&self) -> Result<String, crate::backend::BackendError> {
            self.0.source()
        }
        fn candidates(
            &self,
            path: &[Seg],
        ) -> Result<Option<Vec<Choice>>, crate::backend::BackendError> {
            // Keyed on the relation, not on the index, so the append position
            // is answered by the same arm the third item is.
            Ok(match path.first() {
                Some(Seg::Key(k)) if k == "contents" => Some(vec![
                    Choice::plain("id:prov/1ch2991").detail("prov"),
                    Choice::plain("id:fig/9qk2s1z").detail("fig"),
                ]),
                _ => None,
            })
        }
    }

    fn status_schema() -> Schema {
        use fig_schema::{PathPat, Term};
        Schema::new(vec![
            FieldRule::new(PathPat::key("status")).constraint(Constraint::Enum {
                values: vec![
                    Term::value("active").description("being worked on"),
                    Term::value("archived").retired(true),
                ],
                closed: true,
            }),
            FieldRule::new(PathPat::each_item_of("audience")).constraint(Constraint::Enum {
                values: vec![Term::value("public"), Term::value("private")],
                closed: true,
            }),
        ])
    }

    #[test]
    fn an_enum_field_offers_its_terms_and_a_retired_one_says_so() {
        let backend = FigBackend::open(
            b"status = \"active\"\naudience = [\"public\"]\n",
            Format::Toml,
        )
        .expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_schema(status_schema());

        let choices = model
            .choices_at(&[Seg::Key("status".into())])
            .expect("a vocabulary");
        assert_eq!(
            choices.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            ["active", "archived"]
        );
        assert_eq!(choices[0].detail.as_deref(), Some("being worked on"));
        // Retired, and still offered: a document already holding one has to be
        // able to re-choose it without retyping.
        assert_eq!(choices[1].detail.as_deref(), Some("retired"));

        // An each-item rule answers for an item…
        let item = [Seg::Key("audience".into()), Seg::Index(0)];
        assert_eq!(model.choices_at(&item).map(|c| c.len()), Some(2));
        // …for the append position, which resolves to nothing…
        let append = [Seg::Key("audience".into()), Seg::Index(1)];
        assert_eq!(model.choices_at(&append).map(|c| c.len()), Some(2));
        // …and for the list itself, through the same placeholder.
        assert_eq!(
            model
                .choices_at(&[Seg::Key("audience".into())])
                .map(|c| c.len()),
            Some(2)
        );
        // A field nothing governs has nothing to offer.
        assert!(model.choices_at(&[Seg::Key("nope".into())]).is_none());
    }

    #[test]
    fn the_picker_filters_commits_and_falls_back_to_free_text() {
        let backend = FigBackend::open(b"status = \"active\"\ntitle = \"a note\"\n", Format::Toml)
            .expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_schema(status_schema());
        model.focus_on(&[Seg::Key("status".into())]);

        model.begin_choose();
        assert!(matches!(model.mode, Mode::Choosing { .. }));
        assert_eq!(model.visible_choices().len(), 2);
        assert_eq!(
            model.choice_selected().map(|c| c.label.as_str()),
            Some("active")
        );
        model.choose_next();
        assert_eq!(
            model.choice_selected().map(|c| c.label.as_str()),
            Some("archived")
        );
        model.choose_prev();

        // Narrowing is a case-insensitive substring of the label, and puts the
        // cursor back on a row that is still there.
        for c in "ARCH".chars() {
            model.choose_push(c);
        }
        assert_eq!(model.visible_choices().len(), 1);
        assert_eq!(
            model.choice_selected().map(|c| c.label.as_str()),
            Some("archived")
        );
        model.choose_backspace();
        assert_eq!(model.visible_choices().len(), 1);

        model.choose_commit();
        assert!(matches!(model.mode, Mode::Normal));
        assert!(model.source_snapshot().contains("status = \"archived\""));
        // A retired term is a member, so it applies with a warning rather than
        // being refused.
        assert!(model.status.contains("retired"), "{}", model.status);

        // Cancelling writes nothing.
        let before = model.source_snapshot();
        model.begin_choose();
        model.choose_cancel();
        assert_eq!(model.source_snapshot(), before);

        // And a field with no vocabulary falls through to the text field, so a
        // host binds one key for both.
        model.focus_on(&[Seg::Key("title".into())]);
        model.begin_choose();
        assert!(matches!(
            model.mode,
            Mode::Editing {
                slot: EditSlot::Value,
                ..
            }
        ));
    }

    #[test]
    fn a_backend_answers_for_a_link_field_the_schema_cannot() {
        let backend = WithCandidates(
            FigBackend::open(b"contents = [\"id:prov/1ch2991\"]\n", Format::Toml).expect("open"),
        );
        let mut model = Model::new(backend).expect("model");
        let item = [Seg::Key("contents".into()), Seg::Index(0)];

        let choices = model.choices_at(&item).expect("the workspace answered");
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[1].detail.as_deref(), Some("fig"));
        // The append position is the same question with a different index.
        assert!(
            model
                .choices_at(&[Seg::Key("contents".into()), Seg::Index(1)])
                .is_some()
        );
        // A path the host does not answer for has no picker.
        assert!(model.choices_at(&[Seg::Key("title".into())]).is_none());

        model.focus_on(&item);
        model.begin_choose();
        for c in "fig".chars() {
            model.choose_push(c);
        }
        model.choose_commit();
        assert!(model.source_snapshot().contains("id:fig/9qk2s1z"));
    }

    #[test]
    fn a_filter_that_matches_nothing_commits_nothing() {
        let backend = FigBackend::open(b"status = \"active\"\n", Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_schema(status_schema());
        model.focus_on(&[Seg::Key("status".into())]);
        model.begin_choose();
        for c in "zzz".chars() {
            model.choose_push(c);
        }
        assert!(model.visible_choices().is_empty());
        model.choose_commit();
        assert_eq!(model.status, "nothing matches");
        assert!(model.source_snapshot().contains("status = \"active\""));
        assert!(!model.dirty);
    }

    // ── annotations ───────────────────────────────────────────────────────

    #[test]
    fn a_finding_marks_the_row_it_names_and_survives_an_edit() {
        use crate::annotate::Severity;
        let mut model = sample_model();
        let port = vec![Seg::Key("server".into()), Seg::Key("port".into())];
        let tags = vec![Seg::Key("server".into()), Seg::Key("tags".into())];
        model.set_annotations(vec![
            Annotation::error(port.clone(), "already in use"),
            Annotation::warning(tags.clone(), "two of these are retired"),
        ]);

        let page = model.page_at(&[Seg::Key("server".into())]);
        let at = |path: &[Seg]| {
            page.items
                .iter()
                .find(|i| i.path == path)
                .unwrap_or_else(|| panic!("no item for {path:?}"))
                .annotation
                .clone()
        };
        assert_eq!(at(&port).map(|a| a.severity), Some(Severity::Error));
        assert_eq!(
            at(&tags).map(|a| a.message),
            Some("two of these are retired".to_string())
        );
        // A finding on the list marks the list, and not each of its items.
        let mut first = tags.clone();
        first.push(Seg::Index(0));
        assert_eq!(at(&first), None);
        // …though a caller asking about the item is told what governs it.
        assert_eq!(
            model.annotation_at(&first).map(|a| a.severity),
            Some(Severity::Warning)
        );

        // They are the host's state, not the document's: an edit re-attaches
        // them rather than clearing them.
        model.set_scalar_text(&port, "9090");
        let page = model.page_at(&[Seg::Key("server".into())]);
        assert!(
            page.items
                .iter()
                .any(|i| i.path == port && i.annotation.is_some())
        );
        assert_eq!(model.annotations().len(), 2);

        // The tree projection is marked the same way.
        model.select_row(0);
        assert!(
            model
                .rows
                .iter()
                .any(|r| r.path == port && r.annotation.is_some())
        );

        model.set_annotations(Vec::new());
        assert!(model.rows.iter().all(|r| r.annotation.is_none()));
    }

    // ── undo and redo ─────────────────────────────────────────────────────

    /// The sequence every round-trip test below drives: one op of each kind
    /// that does not delete a node, so the source is expected back byte for
    /// byte. Returns nothing — the assertions are the caller's.
    fn edit_everything(model: &mut Model<FigBackend>) {
        let server = |k: &str| vec![Seg::Key("server".into()), Seg::Key(k.into())];
        model.set_scalar_text(&server("host"), "example.com");
        model.set_value_at(&[Seg::Key("version".into())], Value::Int(2));
        model.append_item_text(&server("tags"), "gamma");
        model.insert_key_text(&server("limits"), "burst", "5");
        model.set_trailing_comment(&server("port"), Some("dev only"));
        model.set_leading_comment(&[Seg::Key("title".into())], Some("renamed"));
        let mut tags = server("tags");
        model.select_row(0);
        model.focus_on(&tags);
        tags.push(Seg::Index(0));
        model.focus_on(&tags);
        model.move_selected_down();
    }

    #[test]
    fn undoing_every_edit_leaves_the_document_that_was_opened() {
        let mut model = sample_model();
        let opened = model.value.clone();
        edit_everything(&mut model);
        assert_ne!(model.value, opened, "the edits did something");
        assert_eq!(model.history_len(), 7);

        while model.history_len() > 0 {
            model.undo();
        }
        assert_eq!(model.value, opened, "back to the value tree it opened with");
        // Nothing was deleted, so the bytes come back too — the comments, the
        // key order, and the blank lines included.
        assert_eq!(model.source_snapshot(), SAMPLE);
        // And undoing back to what was saved reads as clean, however deep the
        // journal got on the way.
        assert!(!model.dirty);
    }

    /// A rename undoes by *value* and not always by bytes: fig's editor writes
    /// the key back through its own quoting rules, so a bare `enabled` renamed
    /// away and back can return as `"enabled"`. The task that asked for this
    /// journal says so — a byte-exact undo is a splice log in fig, not an
    /// inverse op here.
    #[test]
    fn a_rename_undoes_to_the_same_key_if_not_always_the_same_spelling() {
        let mut model = sample_model();
        let opened = model.value.clone();
        model.rename_key(&[Seg::Key("enabled".into())], "on");
        assert!(model.value_at(&[Seg::Key("on".into())]).is_some());
        model.undo();
        assert_eq!(model.value, opened);
        assert!(model.source_snapshot().contains("enabled"));
    }

    #[test]
    fn redo_replays_to_the_bytes_the_edits_produced() {
        let mut model = sample_model();
        edit_everything(&mut model);
        let edited = model.source_snapshot();
        let depth = model.history_len();

        for _ in 0..depth {
            model.undo();
        }
        assert_eq!(model.redo_len(), depth);
        for _ in 0..depth {
            model.redo();
        }
        assert_eq!(model.source_snapshot(), edited);
        assert_eq!(model.history_len(), depth);
        assert_eq!(model.redo_len(), 0);
    }

    #[test]
    fn a_fresh_edit_clears_what_was_undone() {
        let mut model = sample_model();
        model.set_value_at(&[Seg::Key("version".into())], Value::Int(2));
        model.undo();
        assert_eq!(model.redo_len(), 1);
        model.set_value_at(&[Seg::Key("version".into())], Value::Int(3));
        assert_eq!(model.redo_len(), 0);
        model.redo();
        assert_eq!(model.status, "nothing to redo");
        assert_eq!(
            model.value_at(&[Seg::Key("version".into())]),
            Some(&Value::Int(3))
        );
    }

    #[test]
    fn a_deleted_entry_comes_back_with_its_comment_and_its_position() {
        let mut model = sample_model();
        let opened = model.value.clone();
        // A mapping entry, whose leading comment is the document's own note on
        // it, and a sequence item, which comes back by index.
        model.select_row(0);
        model.focus_on(&[Seg::Key("title".into())]);
        model.delete_selected();
        let tags = vec![Seg::Key("server".into()), Seg::Key("tags".into())];
        let mut first = tags.clone();
        first.push(Seg::Index(0));
        model.focus_on(&first);
        model.delete_selected();
        assert_eq!(model.seq_len(&tags), 1);

        model.undo();
        model.undo();
        assert_eq!(model.value, opened, "both nodes back, in their old places");
        assert_eq!(
            model
                .leading_comment_at(&[Seg::Key("title".into())])
                .as_deref(),
            Some("flower sample config — comments and formatting below should survive edits"),
        );
    }

    #[test]
    fn a_derived_key_declines_the_undo_as_it_declined_the_edit() {
        let backend = FigBackend::open(SAMPLE.as_bytes(), Format::Toml).expect("open backend");
        let mut model = Model::with_managed(backend, Vec::new(), vec!["version".to_string()])
            .expect("build model");
        let before = model.source_snapshot();

        model.set_value_at(&[Seg::Key("version".into())], Value::Int(2));
        assert!(model.status.starts_with("rejected:"), "{}", model.status);
        // Declined edits are not edits: there is nothing to undo, and undoing
        // does nothing.
        assert_eq!(model.history_len(), 0);
        assert_eq!(model.edit_seq(), 0);
        model.undo();
        assert_eq!(model.status, "nothing to undo");
        model.redo();
        assert_eq!(model.status, "nothing to redo");
        assert_eq!(model.source_snapshot(), before);
        assert!(!model.dirty);
    }

    #[test]
    fn the_sequence_number_advances_on_every_change_in_either_direction() {
        let mut model = sample_model();
        assert_eq!(model.edit_seq(), 0);
        model.set_value_at(&[Seg::Key("version".into())], Value::Int(2));
        assert_eq!(model.edit_seq(), 1);
        model.undo();
        assert_eq!(model.edit_seq(), 2, "an undo is a change, not a rewind");
        model.redo();
        assert_eq!(model.edit_seq(), 3);
        // A refusal is not a change.
        model.undo();
        model.undo();
        assert_eq!(model.edit_seq(), 4);
    }

    #[test]
    fn a_save_is_not_a_history_boundary() {
        let mut model = sample_model();
        model.set_value_at(&[Seg::Key("version".into())], Value::Int(2));
        model.mark_saved();
        assert!(!model.dirty);
        model.set_value_at(&[Seg::Key("version".into())], Value::Int(3));
        assert!(model.dirty);

        // Undo runs back through the save — and past it, into edits made before
        // the document was written.
        model.undo();
        assert!(!model.dirty, "back at the saved bytes");
        model.undo();
        assert!(model.dirty, "and before them, which is a change again");
        assert_eq!(model.source_snapshot(), SAMPLE);
    }

    #[test]
    fn undo_and_redo_say_whether_the_document_moved() {
        let mut model = Model::new(FigBackend::open(b"a = 1\n", Format::Toml).unwrap()).unwrap();
        assert!(!model.undo(), "nothing to undo yet");
        assert!(!model.redo(), "nothing to redo yet");
        model.set_value_at(&[Seg::Key("a".into())], Value::Int(2));
        assert!(model.undo(), "the edit was there to undo");
        assert!(!model.undo(), "and only once");
        assert!(model.redo(), "the undone edit was there to redo");
        assert!(!model.redo(), "and only once");
    }

    #[test]
    fn undo_puts_the_cursor_back_where_the_edit_was_made() {
        let mut model = sample_model();
        let port = vec![Seg::Key("server".into()), Seg::Key("port".into())];
        model.focus_on(&port);
        model.set_scalar_text(&port, "9090");
        // Somewhere else entirely, the way a user would be by the time they
        // reach for undo.
        model.focus_on(&[Seg::Key("title".into())]);
        model.undo();
        assert_eq!(model.selected_path().as_deref(), Some(&port[..]));
    }
}
