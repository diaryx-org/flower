//! The **page** projection: one level of the document at a time.
//!
//! [`tree`](crate::tree) answers "what does the whole document look like?" — every
//! visible node, indented by depth. That is the right shape for a small config and
//! the wrong one for a deep config, where the useful levels drift right until the
//! keys no longer fit and every screen is mostly ancestors you already know about.
//!
//! A page answers a narrower question: "what is *in* this container?" It lists one
//! container's children and nothing below them, so depth costs a navigation step
//! instead of a column of indentation, and a document nested twelve deep renders
//! exactly as wide as one nested twice. It is the model behind a settings menu —
//! a stable list of categories, and a page you push into and pop back out of.
//!
//! ## Inline vs. drill
//!
//! Listing one level mechanically would be a poor settings menu: a two-key group
//! would cost a whole page to show two lines, and you would spend the interaction
//! budget on containers rather than values. Real settings menus don't do that;
//! they inline the small groups and reserve a page for the substantial ones.
//!
//! So a container is **inlined** into its parent's page — a titled group, its
//! members listed underneath — when its whole subtree fits an [`InlineBudget`]
//! ([`inlines`]); otherwise it becomes a **drill** row that opens a page of its
//! own. The test is deliberately structural rather than schema-driven: flower has
//! to be useful on a document nobody has described. A [`Schema`](crate::Schema)
//! can supersede it later — declared group titles, ordering, an "advanced"
//! section — and feed the same renderer, because the shape it produces is the
//! same.
//!
//! How much fits is the embedder's call, because it is a fact about the room
//! rather than about the document: a frontmatter panel showing one small file
//! wants the whole document on one page, and a deep CI config read in a narrow
//! pane wants a page per level. The budget's per-subtree limits are a row count
//! — the honest cost of inlining, since every descendant is a row — and a
//! depth, the number of ranks of nesting a page is willing to draw. The default
//! ([`InlineBudget::default`]) is the founding rule: at most [`INLINE_MAX`]
//! members, one rank — a small, all-scalar group and nothing else.
//!
//! The decision is per **subtree**, made once at the top: a container either
//! fits entirely or drills. Nothing inside an inlined subtree drills, so a page
//! never nests navigation inside itself, and editing one field can only change
//! how that field's own container renders — never a neighbour's.
//!
//! ## What the page can afford
//!
//! A per-subtree budget is blind to how many subtrees there are, and a list is
//! where that blindness shows. Twenty-two entries of three scalars each pass it
//! twenty-two times over and put eighty rows on one page: four screens of group
//! rules, where the thing a list is *for* — reading one entry against the next —
//! needed one. Every individual yes was right and the page is still wrong.
//!
//! So a sequence is asked a second question, once for all of it
//! ([`InlineBudget::page_rows`]): do the rows its items would contribute fit the
//! room a page has? When they don't, the list is a list — one titled, summarised
//! row per entry, which is the rendering that makes entries comparable anyway.
//! Only sequences are held to it. Their answer is already all-or-nothing
//! ([`seq_inlines`]), where a mapping's children are decided one at a time and a
//! running total would inline whichever key happened to be written first.
//!
//! ## Fitting the room
//!
//! Both limits above are constants, and a constant cannot be right about a room
//! it has never seen. [`InlineBudget::fitting`] asks the document and the room
//! instead: a document that fits entirely is *drawn* entirely, so a file nobody
//! needs to navigate costs no navigation. That is the general form of the
//! observation that a file which is only one array belongs on one page — the
//! shape of the document is not what settles it, the size of it against the
//! room is.
//!
//! A document that does not fit is still drawn in the room it has. The
//! per-subtree limit becomes a share of the room ([`FIT_SHARE`]) rather than
//! the founding constant, so a tall terminal inlines the eleven-member list
//! and the seven-field group that a short one drills, and the page's limit
//! becomes the room itself. Six rows was never a fact about groups; it is a
//! third of the body of a 24-row terminal, which is the room the founding rule
//! was written in. The insets are bounded by [`FIT_MAX_DEPTH`] either way: a
//! group sits one rank under its page, so it may nest one rank less than a
//! document drawn whole, and the page never draws deeper than that document.
//!
//! Inlining is a *presentation* default, never a cage: a group header keeps its
//! own path, so it stays selectable, deletable, and openable as a page like any
//! other container.
//!
//! ## Compression
//!
//! Inlining handles a container too *small* to deserve a page. The opposite
//! shape needs handling too: a container whose single child is a map, which
//! cannot inline (it is not all scalars) and so earns a page — with one row on
//! it, naming the thing you just tapped.
//!
//! The rule that rejects a page for a group header rejects this one for the same
//! reason: a container is worth a page when the page tells you something, and a
//! page listing one drill row does not. So such a row **compresses**: `exports`
//! holding only `journal` renders as one row reading `exports › journal`, and
//! opening it lands on `journal`'s page. The chain is followed as far as it goes
//! ([`PageItem::descend_to`]), through sequence indices as well as keys.
//!
//! It is one row, but it is not a new kind of node. Its
//! [`path`](PageItem::path) is still the outermost container, so every op takes
//! it unchanged and none of them needed a special case — deleting a row that
//! reads `exports › journal` removes the whole chain, which is what it says it
//! is, and leaves no empty `exports` behind. Only opening reads `descend_to`.
//!
//! Backing out retraces it: [`Model::page_back`](crate::Model::page_back) walks
//! out past every level a row compressed past, so leaving costs the step that
//! arriving cost. Popping one raw segment instead would land on the page the
//! compression existed to skip — one row, naming the place you just left — and
//! make the way out twice as long as the way in.
//!
//! The container the row named keeps every op regardless, because the row keeps
//! its [`path`](PageItem::path): renaming, deleting or adding to `exports` are
//! that row's ops on the page you land on. Compression makes a page cheaper to
//! reach and its container no harder to operate.
//!
//! ## Demotion
//!
//! Inlining decides how much room a field gets; **demotion** decides how far up
//! it sits. A document can carry fields nobody came here to type in — a hash the
//! workspace recomputes on every write, a relation the sidebar owns, an identity
//! nothing hand-edits — and listing them among the fields that *are* typed in
//! makes the reader scan past them every time.
//!
//! Hiding them is the wrong answer: a field you can see in the file and not in
//! the editor reads as data loss. So an embedder names those top-level keys
//! ([`Model::set_demoted`](crate::Model::set_demoted)) and they render below the
//! rest, marked [`PageItem::demoted`] — present, editable by whatever owns them,
//! and out of the way. [`Page::partitioned`] is the fold.
//!
//! Demotion is a property of the whole subtree, not of the row: open a demoted
//! container and its page is demoted too ([`Page::demoted`]). A section that
//! stopped being "advanced" one level in would be a section only at the root.

use std::collections::{HashMap, HashSet};

use fig::Value;
pub use fig_schema::Seg;

use crate::tree::{VKind, key_to_string, preview, value_at};

/// The row limit of the **default** [`InlineBudget`].
///
/// Six is the point where a group stops reading as a handful of related fields
/// and starts reading as a list — and where inlining two of them in a row would
/// fill a short terminal with somebody else's fields. It is a presentation
/// constant, not a correctness one: raising it inlines more, lowering it drills
/// more, and nothing else changes.
pub const INLINE_MAX: usize = 6;

/// The row limit of the **default** [`InlineBudget`]'s *page*.
///
/// Twenty is about a short terminal's body: the point past which a list has
/// stopped being something you read one entry of against the next, and become
/// something you scroll. It bounds what [`INLINE_MAX`] cannot — a list of
/// entries each small enough to inline and numerous enough that inlining all of
/// them buries the page.
pub const PAGE_INLINE_MAX: usize = 20;

/// The deepest document [`InlineBudget::fitting`] will pour onto one page —
/// and, less the rank a group sits under its page, the deepest subtree it will
/// inline into one when the document as a whole does not fit.
///
/// Rows are not the only thing a page spends: every rank of nesting is two more
/// columns of inset on every row below it. A document that fits vertically and
/// runs eight ranks deep fits by the row count and not by the eye, so past this
/// it drills however short it is.
pub const FIT_MAX_DEPTH: usize = 3;

/// The share of the room one inlined subtree may take, when the document does
/// not fit the room whole: a third.
///
/// [`INLINE_MAX`] is this share of a 24-row terminal's body, which is the room
/// the founding rule was written in — so [`InlineBudget::fitting`] reproduces
/// that rule there, and in a taller terminal inlines what the same eye would
/// still read as a group rather than as a page. A subtree that takes half the
/// room is the page; one that takes a third is a group on it. Never less than
/// [`INLINE_MAX`], so a short terminal loses nothing it had.
pub const FIT_SHARE: usize = 3;

/// How much of a container's subtree may be inlined into its parent's page
/// rather than drilled into — the knob that slides the page projection between
/// its two ancestors.
///
/// At the default, a page is a settings menu: small all-scalar groups inline,
/// everything substantial earns a page. Raised far enough, the root page simply
/// *is* the whole document — the settings-list rendering, absorbed — and a
/// small document never asks for a navigation step at all. The embedder picks,
/// because the right answer is about the room the pages are drawn in, not
/// about the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineBudget {
    /// The most rows an inlined subtree may contribute to the page. Every
    /// descendant is a row — nested containers add their headers too — so this
    /// is the honest cost of saying yes.
    pub rows: usize,
    /// The most ranks of nesting an inlined subtree may reach: 1 admits only
    /// all-scalar groups, 2 lets those groups hold one more rank of groups, and
    /// so on. Rendered as [`PageItem::inset`], so this bounds the indentation a
    /// page can ask a frontend to draw.
    pub depth: usize,
    /// The most rows one page will spend on the items of a sequence it inlines
    /// ([`seq_inlines`]) — the limit [`rows`](Self::rows) cannot express,
    /// because that one is asked once per item and this one once per page.
    ///
    /// Twenty-two entries of three scalars each pass a per-item budget
    /// individually and put eighty rows on one page between them. Every
    /// individual yes was right and the page is still wrong, so the page gets a
    /// say of its own.
    pub page_rows: usize,
}

impl InlineBudget {
    /// A budget from the two per-subtree limits, with a page limit at least as
    /// generous as [`PAGE_INLINE_MAX`].
    ///
    /// Raising `rows` past it raises the page's limit with it: a caller asking
    /// for a hundred rows of subtree is asking for a page that can hold them,
    /// and a page cap left at the default would refuse what the caller just
    /// paid for.
    pub fn new(rows: usize, depth: usize) -> Self {
        Self {
            rows,
            depth,
            page_rows: rows.max(PAGE_INLINE_MAX),
        }
    }

    /// The budget that shows as much of `root` as `room` rows allow.
    ///
    /// The founding rule is a constant, and a constant cannot be right about a
    /// room it has never seen: six rows and one rank is a wise default over a
    /// deep CI config in a narrow pane, and a needless navigation step over a
    /// document that would have fit on screen whole. This asks the document and
    /// the room instead.
    ///
    /// Two answers. If the whole document fits — few enough rows, and shallow
    /// enough ([`FIT_MAX_DEPTH`]) that the insets stay readable — the budget is
    /// the document's own size and the root page simply *is* the document: no
    /// navigation at all for a file that never needed any, which is the general
    /// form of "a file that is only one array belongs on one page". Otherwise
    /// every limit is the room's: a subtree may take a share of it
    /// ([`FIT_SHARE`]), nested as deep as a whole document's subtrees may be,
    /// and a page may spend all of it on one list. So a taller terminal
    /// inlines the group and the list that a short one drills, and a short one
    /// is exactly the founding rule, which is where that rule's constant came
    /// from.
    ///
    /// Measured over the whole document, hidden keys included: they are the
    /// embedder's few reserved names, and counting them costs at most a row of
    /// slack in a heuristic that is choosing between two roundings anyway.
    pub fn fitting(root: &Value, room: usize) -> Self {
        let (rows, depth) = subtree_shape(root);
        if rows > 0 && rows <= room && depth <= FIT_MAX_DEPTH {
            return Self {
                rows,
                depth,
                page_rows: rows,
            };
        }
        // One rank less than a whole document: the subtree's header is a row
        // on the page, so its members sit one rank deeper than the document's
        // own would, and the page's insets stay within what a whole document
        // may draw.
        Self {
            rows: (room / FIT_SHARE).max(INLINE_MAX),
            depth: FIT_MAX_DEPTH - 1,
            page_rows: room.max(INLINE_MAX),
        }
    }
}

impl Default for InlineBudget {
    /// The founding rule: at most [`INLINE_MAX`] members, all of them scalars,
    /// and at most [`PAGE_INLINE_MAX`] rows of them on any one page.
    fn default() -> Self {
        Self::new(INLINE_MAX, 1)
    }
}

/// What a [`PageItem`] does when you activate it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// A leaf: editable in place.
    Scalar,
    /// A container substantial enough to earn its own page. `count` is how many
    /// children it holds — what a "12 fields ›" affordance shows.
    Drill { count: usize },
    /// The title of a container inlined into *this* page. The items that follow it
    /// at [`PageItem::inset`] 1 are its members.
    ///
    /// Selectable, and openable as a page in its own right: the inline rendering
    /// is a default, not a restriction.
    GroupHeader { count: usize },
}

/// One line of a page.
#[derive(Clone, Debug)]
pub struct PageItem {
    /// The fig path to this node from the document root — the same currency
    /// [`tree`](crate::tree) deals in, so an edit op takes it unchanged.
    pub path: Vec<Seg>,
    /// The mapping key, or `[i]` for a sequence item.
    pub label: String,
    pub vkind: VKind,
    /// A one-line rendering of the value (the scalar text, or `{n}` / `[n]`).
    pub preview: String,
    pub kind: ItemKind,
    /// 0 for a direct child of the page's focus; 1 for a member of a group
    /// inlined into it, and one more for each further rank the [`InlineBudget`]
    /// admitted. Bounded by [`InlineBudget::depth`], so the default budget never
    /// goes past 1.
    pub inset: usize,
    /// A readable stand-in for a sequence item's index — the value of whichever
    /// of its fields best names it ([`title_keys`]). `None` for a mapping entry,
    /// whose key already names it, and for an item nothing distinguishes.
    ///
    /// It never replaces [`label`](Self::label): the index is what the path is
    /// addressed by and what a reorder moves, so a frontend shows both.
    pub title: Option<String>,
    /// Where opening this row lands, when the pages between here and there would
    /// each list nothing but the next step down.
    ///
    /// Equal to [`path`](Self::path) for almost every row. It differs for a
    /// **compressed** drill — `exports › journal`, one row standing for a chain
    /// of containers that hold only each other — where it is the deepest of
    /// them, the first one whose page has something to say.
    ///
    /// [`path`](Self::path) stays the outermost node, so every op still takes it
    /// unchanged and none of them needed a special case: deleting the row
    /// removes the whole chain (which is what deleting something called
    /// `exports › journal` should do, and leaves no empty husk behind), and
    /// renaming it renames `exports`. Only *opening* looks here.
    pub descend_to: Vec<Seg>,
    /// Whether this item belongs *below* the fields a reader came here to edit
    /// — a page's own "advanced" section, in the sense a settings menu means it.
    ///
    /// Set by the embedder's demoted-key set, and root-scoped exactly as
    /// [`build_page`]'s hiding is: it is a property of the whole subtree under a
    /// top-level key, so every item on a demoted container's page is demoted too
    /// and the section cannot come apart when you drill into it.
    ///
    /// A demotion, not a hiding and not a lock: the item renders, carries its
    /// path, and takes every op the others take. It says only that a reader
    /// scanning for the field they meant to change should not have to read past
    /// this one to find it.
    pub demoted: bool,
    /// A container's entire contents in flow form (`{branches: [main]}`), when
    /// they are short enough to be worth showing instead of counting.
    ///
    /// `1 field ›` is strictly less than the document says: the field is right
    /// there and it fits. A count is what you fall back to when the contents
    /// don't ([`SUMMARY_BUDGET`]), not the default way to describe a small
    /// container. `None` for a scalar, whose value is already its own row.
    pub summary: Option<String>,
    /// The own-line comment block written above this node in the document,
    /// lines joined by `\n`, markers stripped — what the file says *about* the
    /// entry, which is the closest thing an undescribed document has to a
    /// schema's help text. A renderer shows it under (or over) the name, and a
    /// host with a schema description for the field lets that win.
    ///
    /// Not [`build_page`]'s to fill: the value tree carries no comments, so
    /// the model asks the backend after the page is built, and a page built
    /// from a bare `Value` has `None` throughout.
    pub leading_comment: Option<String>,
    /// The same-line comment after this node's value (`port = 8080 # dev`),
    /// marker stripped. Filled the same way as
    /// [`leading_comment`](Self::leading_comment).
    pub trailing_comment: Option<String>,
    /// What the host has to say about this node — a broken link, a duplicate
    /// id, anything only a workspace can know ([`annotate`](crate::annotate)).
    ///
    /// Addressed **exactly**: a finding on a list marks the list's row and not
    /// each of its items, which would put one error's glyph on ninety-five
    /// rows. An item is marked when the host says so about the item.
    ///
    /// Not [`build_page`]'s to fill either — the same pass that reads comments
    /// attaches these, for the same reason: the value tree holds neither.
    pub annotation: Option<crate::annotate::Annotation>,
}

impl PageItem {
    /// Whether activating this item opens a page (rather than editing a value).
    ///
    /// A group header does **not**, though it names a container: its members are
    /// already on this page, so the page it would open shows exactly what you can
    /// already see — the same two rows twice, once on each side of a split. A
    /// container is worth a page when the page tells you something; this one
    /// cannot. Its members are reached by moving onto them, and every op that
    /// takes the group itself takes a path, which the header still carries.
    pub fn is_drill(&self) -> bool {
        matches!(self.kind, ItemKind::Drill { .. })
    }

    /// Whether this item names a container at all — a drill row, or the header of
    /// a group inlined into this page.
    pub fn is_container(&self) -> bool {
        matches!(
            self.kind,
            ItemKind::Drill { .. } | ItemKind::GroupHeader { .. }
        )
    }

    pub fn is_scalar(&self) -> bool {
        matches!(self.kind, ItemKind::Scalar)
    }

    /// Whether this item's *label* can be changed — true for a mapping entry,
    /// false for a sequence item.
    ///
    /// A sequence item's label is its index: it is the position, not a name, so
    /// there is nothing to rename and the only thing that moves it is a reorder.
    /// The inference is one line, which is exactly why it belongs here — every
    /// frontend that redid it would be one edit away from disagreeing with the
    /// op that actually refuses.
    pub fn can_rename(&self) -> bool {
        matches!(self.path.last(), Some(Seg::Key(_)))
    }

    /// Whether this row stands for a chain of containers rather than for one
    /// ([`descend_to`](Self::descend_to)).
    pub fn is_compressed(&self) -> bool {
        self.descend_to.len() > self.path.len()
    }

    /// The names this row shows, outermost first — `["exports", "journal"]` for a
    /// compressed drill, and just the label for every other row. A frontend joins
    /// them with whatever separator its breadcrumb uses.
    pub fn chain_labels(&self) -> Vec<String> {
        std::iter::once(self.label.clone())
            .chain(
                self.descend_to[self.path.len().min(self.descend_to.len())..]
                    .iter()
                    .map(seg_label),
            )
            .collect()
    }
}

/// One container's children, ready to render.
#[derive(Clone, Debug, Default)]
pub struct Page {
    /// The container being listed. Empty is the document root.
    pub focus: Vec<Seg>,
    pub items: Vec<PageItem>,
    /// What this page's own container is called, when it is a sequence item and
    /// its index is not worth reading — the same title its row carried on the
    /// page you opened it from, so the breadcrumb agrees with what you clicked.
    pub title: Option<String>,
    /// Whether this whole page sits under a demoted top-level key.
    ///
    /// The page you reach by opening a demoted row. A frontend that folds its
    /// demoted items behind an "advanced" disclosure reads this to keep the
    /// framing once you are inside — the section a page came out of is still
    /// true of the page.
    pub demoted: bool,
}

impl Page {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Where `path` sits in this page, if it is on it.
    ///
    /// A compressed row answers for its whole chain: both the node it *is*
    /// (`exports`) and the node it *opens* (`exports.journal`) find it, because
    /// every caller is asking the same question — which row here corresponds to
    /// that node — and for a chain the answer is the one row standing for all of
    /// it. Identical to matching on the path alone for any row that is not
    /// compressed, where the two are the same.
    pub fn position_of(&self, path: &[Seg]) -> Option<usize> {
        self.items
            .iter()
            .position(|i| i.path == path || i.descend_to == path)
    }

    /// Whether any item on this page opens a page of its own.
    ///
    /// A page with none is a leaf of the navigation, and — at the root — a
    /// document with no depth to navigate at all, which is how a frontend knows
    /// to spend the whole width on one pane instead of drawing an empty second
    /// one. See [`Model::pages_would_degenerate`](crate::Model::pages_would_degenerate).
    pub fn has_drills(&self) -> bool {
        self.items.iter().any(PageItem::is_drill)
    }

    /// Whether this page offers a choice — two rows or more.
    ///
    /// What a pane full of it would be *for*. A frontend drawing a sidebar reads
    /// it to find out whether there is anything to select between: one row is a
    /// label, not a menu, and half a screen is a lot to spend on a label.
    pub fn has_choice(&self) -> bool {
        self.items.len() >= 2
    }

    /// The page's items in two stable runs: the ones a reader came to edit, then
    /// the demoted ones.
    ///
    /// [`items`](Self::items) stays in document order, because that order is the
    /// document's and flower does not get to reshuffle it. This is the one
    /// rearrangement a settings menu does want — the "advanced" fold — offered
    /// here rather than left to each frontend so they all fold at the same seam.
    ///
    /// The partition is stable, and demotion is root-scoped, so a group header
    /// and the members inlined under it always land in the same run, adjacent and
    /// in order: the fold can never cut a group in half.
    pub fn partitioned(&self) -> (Vec<&PageItem>, Vec<&PageItem>) {
        self.items.iter().partition(|i| !i.demoted)
    }

    /// The page's title as a breadcrumb — `server › limits`, or `root_label` for
    /// the document root.
    pub fn breadcrumb(&self, root_label: &str) -> String {
        if self.focus.is_empty() {
            return root_label.to_string();
        }
        let mut parts: Vec<String> = self.focus.iter().map(seg_label).collect();
        if let (Some(title), Some(last)) = (&self.title, parts.last_mut()) {
            *last = title.clone();
        }
        parts.join(" › ")
    }
}

/// How a path segment reads in a breadcrumb or a label.
pub fn seg_label(seg: &Seg) -> String {
    match seg {
        Seg::Key(k) => k.clone(),
        Seg::Index(i) => format!("[{i}]"),
    }
}

/// Whether `v` is a container at all — the test for whether a path can be focused.
pub fn is_container(v: &Value) -> bool {
    matches!(v, Value::Map(_) | Value::Seq(_))
}

/// How many children `v` holds (0 for a scalar).
fn child_count(v: &Value) -> usize {
    match v {
        Value::Map(entries) => entries.len(),
        Value::Seq(items) => items.len(),
        _ => 0,
    }
}

/// Whether `v` is inlined into its parent's page rather than given one of its
/// own: a non-empty container whose whole subtree fits `budget` — few enough
/// rows, and nested no deeper than the budget's rank limit.
///
/// An empty container is excluded deliberately. It has nothing to inline, and a
/// titled group with no members under it reads as a rendering bug; as a drill row
/// it stays visible, countable, and somewhere to add the first key.
pub fn inlines(v: &Value, budget: InlineBudget) -> bool {
    let (rows, depth) = subtree_shape(v);
    rows > 0 && rows <= budget.rows && depth <= budget.depth
}

/// Whether a sequence's items are inlined into its page — all of them, or none.
///
/// Two tests, and a list has to pass both. Every item must fit `budget` on its
/// own (the founding rule, applied item by item), *and* the rows they would
/// contribute between them must fit [`InlineBudget::page_rows`].
///
/// The second is the one a list needs and the per-item test cannot give it,
/// because a per-item test is blind to how many items there are. Twenty-two
/// entries of three scalars each say yes twenty-two times and put eighty rows on
/// one page: four screens of group rules, where the thing a list is *for* —
/// reading one entry against the next — needed one. Drilled instead, the same
/// twenty-two are twenty-two rows, each titled and summarised, and the
/// comparison is back on screen.
///
/// A mapping is under no such rule. Its children have distinct names and are
/// decided one at a time, so a running total would inline whichever happened to
/// be written first and drill the rest — a page whose shape depends on key
/// order, which is not a fact about the document. A sequence can be held to a
/// total precisely because its answer is already all-or-nothing.
fn seq_inlines(items: &[Value], budget: InlineBudget) -> bool {
    let mut rows = 0usize;
    for item in items {
        if is_container(item) {
            if !inlines(item, budget) {
                return false;
            }
            // The header, then everything under it.
            rows += 1 + subtree_shape(item).0;
        } else {
            // A scalar item is one row whatever is decided here — it has no
            // subtree to inline — but it is still a row this page has to draw.
            rows += 1;
        }
    }
    rows <= budget.page_rows
}

/// The rendered cost of inlining `v`: how many rows its subtree would put on
/// the page (every descendant is one — nested containers count their headers
/// too), and how many ranks of inset the deepest of them would wear.
/// `(0, 0)` for a scalar; an empty container is `(0, 1)`, which no budget
/// accepts because there are no rows in it to want.
fn subtree_shape(v: &Value) -> (usize, usize) {
    let children: Box<dyn Iterator<Item = &Value>> = match v {
        Value::Map(entries) => Box::new(entries.iter().map(|(_, c)| c)),
        Value::Seq(items) => Box::new(items.iter()),
        _ => return (0, 0),
    };
    let (mut rows, mut depth) = (0, 0);
    for child in children {
        let (r, d) = subtree_shape(child);
        rows += 1 + r;
        depth = depth.max(d);
    }
    (rows, depth + 1)
}

/// Keys that conventionally name the thing they sit in, best first.
///
/// A small list on purpose. It is a tie-breaker over the structural evidence
/// below, not the mechanism: config files that call it something else are the
/// common case, and a list long enough to cover them would start guessing wrong.
const NAME_KEYS: [&str; 5] = ["name", "title", "id", "label", "key"];

/// Rank the keys of a sequence's items by how well each one *names* an item,
/// best first.
///
/// A sequence of mappings is the one place a config has no names to show: the
/// items are addressed by index, and `[0]`, `[1]`, `[2]` tell you nothing about
/// which step, service, or rule you are looking at. The information is there —
/// it is just in a field rather than in a key — so this works out which field.
///
/// Three signals, in one score:
///
/// - **coverage** — how many items have this key at all, with a scalar value.
/// - **distinctness** — how many of those values differ. A key that reads the
///   same on every item cannot tell them apart, however faithfully it is filled
///   in, so this is weighted hardest.
/// - **convention** — whether it is one of [`NAME_KEYS`].
///
/// A *ranking* rather than a single answer, because items in the same sequence
/// need not have the same keys: a GitHub Actions step is named by `uses` or by
/// `run` depending on which kind of step it is, and each item takes the best
/// key it actually has ([`title_of`]).
pub fn title_keys(items: &[Value]) -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    let mut stats: HashMap<String, (usize, HashSet<String>)> = HashMap::new();
    let mut mappings = 0usize;

    for item in items {
        let Value::Map(entries) = item else { continue };
        mappings += 1;
        for (k, v) in entries {
            if is_container(v) {
                continue;
            }
            let key = key_to_string(k);
            let seen = stats.entry(key.clone()).or_insert_with(|| {
                order.push(key);
                (0, HashSet::new())
            });
            seen.0 += 1;
            seen.1.insert(preview(v));
        }
    }
    if mappings == 0 {
        return Vec::new();
    }

    let mut ranked: Vec<(f64, usize, usize, &String)> = order
        .iter()
        .enumerate()
        .map(|(doc_order, key)| {
            let (present, values) = &stats[key];
            let coverage = *present as f64 / mappings as f64;
            let distinctness = values.len() as f64 / *present as f64;
            let convention = NAME_KEYS.iter().position(|n| n.eq_ignore_ascii_case(key));
            let score =
                coverage + 1.5 * distinctness + if convention.is_some() { 2.0 } else { 0.0 };
            (score, convention.unwrap_or(NAME_KEYS.len()), doc_order, key)
        })
        .collect();
    // Best score first; ties settled by convention, then by the order the
    // document itself puts the keys in — both stable, so a page does not
    // reshuffle its titles when an unrelated field is edited.
    ranked.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    ranked.into_iter().map(|(_, _, _, k)| k.clone()).collect()
}

/// The title `item` takes from a ranking: the value of the best-ranked key it
/// actually has. `None` for a non-mapping, or one with none of the keys.
pub fn title_of(ranking: &[String], item: &Value) -> Option<String> {
    title_entry_of(ranking, item).map(|(_, title)| title)
}

/// [`title_of`], and the key the title came out of.
///
/// The key matters to whoever is about to describe the same mapping a second
/// time on the same row: a summary that repeats the field the title is already
/// showing spends the row's width saying it twice ([`flow_without`]).
pub fn title_entry_of<'r>(ranking: &'r [String], item: &Value) -> Option<(&'r str, String)> {
    let Value::Map(entries) = item else {
        return None;
    };
    ranking.iter().find_map(|want| {
        entries.iter().find_map(|(k, v)| {
            (!is_container(v) && key_to_string(k) == *want).then(|| (want.as_str(), preview(v)))
        })
    })
}

/// How long a container's flow-form summary may get before a count is the more
/// useful thing to show.
///
/// Generous, because the renderer applies the real limit — whatever room the row
/// actually has — and falls back to the count on its own. This only stops the
/// projection building a 4KB string for a container nobody could render anyway.
pub const SUMMARY_BUDGET: usize = 72;

/// A container's whole contents on one line, in flow form, or `None` if they run
/// past `budget`.
///
/// Flow form because that is how the formats themselves write a small container
/// — `{branches: [main]}` is valid YAML, JSON, and (near enough) TOML — so it
/// reads as the document rather than as a rendering of it.
pub fn flow(v: &Value, budget: usize) -> Option<String> {
    let rendered = match v {
        Value::Map(entries) => {
            let parts = entries
                .iter()
                .map(|(k, val)| Some(format!("{}: {}", key_to_string(k), flow(val, budget)?)))
                .collect::<Option<Vec<_>>>()?;
            format!("{{{}}}", parts.join(", "))
        }
        Value::Seq(items) => {
            let parts = items
                .iter()
                .map(|i| flow(i, budget))
                .collect::<Option<Vec<_>>>()?;
            format!("[{}]", parts.join(", "))
        }
        scalar => preview(scalar),
    };
    (rendered.chars().count() <= budget).then_some(rendered)
}

/// [`flow`], with one top-level key left out — the one a row is already showing
/// as its title.
///
/// `[0] · diaryx  {name: diaryx, public: false, lang: rust+swift}` says `diaryx`
/// twice in a row that has room for neither, and the copy it drops is the one
/// the eye already read. An elision, like every summary: the field is on the
/// page the row opens, and the row's count still counts it.
///
/// `None` when nothing is left — a mapping whose only field is its own name has
/// no contents to show beyond the title, and the count says the rest.
fn flow_without(v: &Value, budget: usize, omit: &str) -> Option<String> {
    let Value::Map(entries) = v else {
        return flow(v, budget);
    };
    let parts = entries
        .iter()
        .filter(|(k, _)| key_to_string(k) != omit)
        .map(|(k, val)| Some(format!("{}: {}", key_to_string(k), flow(val, budget)?)))
        .collect::<Option<Vec<_>>>()?;
    if parts.is_empty() {
        return None;
    }
    let rendered = format!("{{{}}}", parts.join(", "));
    (rendered.chars().count() <= budget).then_some(rendered)
}

/// Build the page listing the container at `focus`.
///
/// Two root-scoped key sets shape the result, and they are root-scoped in the
/// same sense but not in the same way:
///
/// - `hidden_top_level` is the hiding [`tree::build_rows`] honors (an embedder's
///   managed keys), applied only when `focus` *is* the root — a hidden key
///   produces no item, and a nested key that happens to share a hidden name is
///   untouched.
/// - `demoted_top_level` marks a key's whole **subtree**
///   ([`PageItem::demoted`]), so it applies at every focus: the items of a
///   demoted container's page are demoted, and so is the page
///   ([`Page::demoted`]). That is what keeps an "advanced" section from coming
///   apart the moment you open something inside it.
///
/// The asymmetry is deliberate. Hiding answers "does this row exist here?",
/// which only the root can ask, since that is the level the embedder reserves
/// keys at. Demotion answers "how prominent is this?", which stays true however
/// deep you go.
///
/// A `focus` that doesn't resolve, or that names a scalar, yields an empty page.
/// The projection stays total so a frontend never has to guard it; the model
/// keeps `focus` on a real container anyway
/// ([`Model::reanchor_focus`](crate::Model)).
pub fn build_page(
    root: &Value,
    focus: &[Seg],
    hidden_top_level: &HashSet<String>,
    demoted_top_level: &HashSet<String>,
    budget: InlineBudget,
) -> Page {
    let mut page = Page {
        focus: focus.to_vec(),
        items: Vec::new(),
        title: page_title(root, focus),
        demoted: under_demoted_root(focus, demoted_top_level),
    };
    let Some(node) = value_at(root, focus) else {
        return page;
    };
    let at_root = focus.is_empty();

    // A sequence's items render alike, whatever their individual sizes.
    //
    // Applying the inline test per item would expand whichever entries happen to
    // be small and collapse the rest — a list where some rows are three lines and
    // others are one, which reads as a rendering fault rather than as a list. It
    // also destroys the one comparison a list is for: entry against entry. So a
    // sequence inlines every mapping item or none ([`seq_inlines`]), and "none"
    // is the answer as soon as one item is too big or too nested to inline — or
    // as soon as there are too many of them to be worth a page between them.
    //
    // A mapping's children are under no such rule: they have distinct names, so
    // a mix of inlined groups and drill rows reads as what it is.
    let (uniform, ranking) = match node {
        Value::Seq(items) => (Some(seq_inlines(items, budget)), title_keys(items)),
        _ => (None, Vec::new()),
    };

    for (label, path, child) in children_of(node, focus) {
        if at_root && hidden_top_level.contains(&label) {
            continue;
        }
        // Every item on this page shares the page's root key when the focus is
        // not the root, so off the root this is just `page.demoted` — one test
        // that reads the same at both levels rather than two that agree by
        // accident.
        let demoted = under_demoted_root(&path, demoted_top_level);
        let title = title_of(&ranking, child);
        if !is_container(child) {
            page.items.push(item(
                label,
                path,
                child,
                ItemKind::Scalar,
                0,
                title,
                demoted,
            ));
        } else if uniform.unwrap_or(true) && inlines(child, budget) {
            push_inline(&mut page.items, label, path, child, 0, title, demoted);
        } else {
            // A drill row stands for everything between here and the first page
            // that has something to say: `exports` holding only `journal` is one
            // row reading `exports › journal`, not two taps through a page whose
            // whole content is a name you just tapped. The row is described by
            // what it lands on — its count, its summary, its kind — while its
            // path stays the outermost node, so every op still takes it
            // unchanged.
            let (descend_to, deep) = compress(&path, child, budget);
            let count = child_count(deep);
            let mut row = item(
                label,
                path,
                deep,
                ItemKind::Drill { count },
                0,
                title,
                demoted,
            );
            // A titled row would otherwise describe itself twice — the title
            // and the summary's first field are the same field — in a row that
            // has room for neither.
            //
            // `child`, not `deep`, and the two are the same whenever this fires:
            // compression needs a container whose *only* child is a container,
            // and a title needs a scalar field, so a row can have one or the
            // other and never both (`compression_and_a_title_cannot_meet`).
            if let Some((key, _)) = title_entry_of(&ranking, child) {
                row.summary = flow_without(child, SUMMARY_BUDGET, key);
            }
            row.descend_to = descend_to;
            page.items.push(row);
        }
    }
    page
}

/// Emit an inlined container: its header, then its whole subtree, each rank one
/// inset deeper.
///
/// No budget here, deliberately: [`inlines`] measured the entire subtree before
/// saying yes, so by the time this runs every node under `v` has already been
/// paid for and nothing inside it can drill. The one thing computed per level is
/// a sequence's title ranking, so a nested list names its items the way it would
/// on a page of its own.
fn push_inline(
    items: &mut Vec<PageItem>,
    label: String,
    path: Vec<Seg>,
    v: &Value,
    inset: usize,
    title: Option<String>,
    demoted: bool,
) {
    let count = child_count(v);
    items.push(item(
        label,
        path.clone(),
        v,
        ItemKind::GroupHeader { count },
        inset,
        title,
        demoted,
    ));
    let ranking = match v {
        Value::Seq(members) => title_keys(members),
        _ => Vec::new(),
    };
    for (sub_label, sub_path, sub) in children_of(v, &path) {
        let sub_title = title_of(&ranking, sub);
        if is_container(sub) {
            push_inline(
                items,
                sub_label,
                sub_path,
                sub,
                inset + 1,
                sub_title,
                demoted,
            );
        } else {
            items.push(item(
                sub_label,
                sub_path,
                sub,
                ItemKind::Scalar,
                inset + 1,
                sub_title,
                demoted,
            ));
        }
    }
}

/// The one child `v` holds, when `v` holds exactly one and that child would be a
/// drill row on `v`'s own page.
///
/// The test for "opening this would show me a page with one row on it". A
/// container with one child that *inlines* fails it: that page lists a group
/// header and its members, which is several rows and a real answer to what is
/// in there. So does a container with one scalar child, for the same reason.
///
/// A sequence is included. Its lone item is a drill by the uniformity rule
/// (nothing to be uniform with, and it does not inline), and `audiences › [0]`
/// is exactly as uninformative a page as the mapping case.
fn lone_drill_child(v: &Value, budget: InlineBudget) -> Option<(Seg, &Value)> {
    let (seg, child) = match v {
        Value::Map(entries) if entries.len() == 1 => {
            let (k, c) = entries.iter().next()?;
            (Seg::Key(key_to_string(k)), c)
        }
        Value::Seq(items) if items.len() == 1 => (Seg::Index(0), items.first()?),
        _ => return None,
    };
    (is_container(child) && !inlines(child, budget)).then_some((seg, child))
}

/// Follow [`lone_drill_child`] as far as it goes, from the container at `base`.
///
/// Returns where opening `v` should land and what is actually there. Terminates
/// because every step is strictly deeper into a finite document.
fn compress<'v>(base: &[Seg], v: &'v Value, budget: InlineBudget) -> (Vec<Seg>, &'v Value) {
    let mut descend_to = base.to_vec();
    let mut deep = v;
    while let Some((seg, next)) = lone_drill_child(deep, budget) {
        descend_to.push(seg);
        deep = next;
    }
    (descend_to, deep)
}

/// Whether the page listing `focus` is one a row compressed past: it holds
/// exactly one item, and that item opens a page of its own.
///
/// The same condition [`compress`] walks, asked from the other end. A frontend
/// that skipped this level going in should not be handed it coming out — see
/// [`Model::parent_page`](crate::Model::parent_page).
pub fn is_compressed_past(
    root: &Value,
    focus: &[Seg],
    hidden: &HashSet<String>,
    budget: InlineBudget,
) -> bool {
    let page = build_page(root, focus, hidden, &HashSet::new(), budget);
    page.items.len() == 1 && page.items[0].is_drill()
}

/// Whether `path` descends from a demoted top-level key.
///
/// The same root-scoped shape as
/// [`Model::is_derived`](crate::Model::is_derived): only the first segment is
/// consulted, and only when it is a key. A sequence at the root has no name to
/// demote by, and a nested `id` under some other key is a user's own field that
/// happens to share a managed key's spelling — neither is what the embedder
/// named.
fn under_demoted_root(path: &[Seg], demoted_top_level: &HashSet<String>) -> bool {
    matches!(path.first(), Some(Seg::Key(k)) if demoted_top_level.contains(k))
}

/// The title of the container `focus` names, when it is a sequence item — the
/// same one its row carried on the page it was opened from.
fn page_title(root: &Value, focus: &[Seg]) -> Option<String> {
    let Some(Seg::Index(i)) = focus.last() else {
        return None;
    };
    let Value::Seq(items) = value_at(root, &focus[..focus.len() - 1])? else {
        return None;
    };
    title_of(&title_keys(items), items.get(*i)?)
}

fn item(
    label: String,
    path: Vec<Seg>,
    v: &Value,
    kind: ItemKind,
    inset: usize,
    title: Option<String>,
    demoted: bool,
) -> PageItem {
    let descend_to = path.clone();
    PageItem {
        path,
        descend_to,
        label,
        vkind: VKind::of(v),
        preview: preview(v),
        kind,
        inset,
        title,
        demoted,
        summary: is_container(v).then(|| flow(v, SUMMARY_BUDGET)).flatten(),
        leading_comment: None,
        trailing_comment: None,
        annotation: None,
    }
}

/// The (label, path, value) of each child of a container, in document order.
/// Empty for a scalar.
fn children_of<'v>(node: &'v Value, base: &[Seg]) -> Vec<(String, Vec<Seg>, &'v Value)> {
    let extend = |seg: Seg| {
        let mut p = base.to_vec();
        p.push(seg);
        p
    };
    match node {
        Value::Map(entries) => entries
            .iter()
            .map(|(k, v)| {
                let key = key_to_string(k);
                (key.clone(), extend(Seg::Key(key)), v)
            })
            .collect(),
        Value::Seq(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("[{i}]"), extend(Seg::Index(i)), v))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fig::Format;

    const SAMPLE: &str = "\
title = \"flower\"
version = 1
enabled = true

[server]
host = \"localhost\"
port = 8080
tags = [\"alpha\", \"beta\"]

[server.limits]
max_connections = 100
timeout = 30.5
";

    fn value_of(src: &str, fmt: Format) -> Value {
        fig::Document::parse(src.as_bytes(), fmt)
            .expect("parse")
            .to_value()
            .expect("to_value")
    }

    fn sample() -> Value {
        value_of(SAMPLE, Format::Toml)
    }

    fn page_of(root: &Value, focus: &[Seg]) -> Page {
        build_page(
            root,
            focus,
            &HashSet::new(),
            &HashSet::new(),
            InlineBudget::default(),
        )
    }

    fn budgeted(root: &Value, focus: &[Seg], budget: InlineBudget) -> Page {
        build_page(root, focus, &HashSet::new(), &HashSet::new(), budget)
    }

    fn demoting(root: &Value, focus: &[Seg], demoted: &[&str]) -> Page {
        let set: HashSet<String> = demoted.iter().map(|s| s.to_string()).collect();
        build_page(root, focus, &HashSet::new(), &set, InlineBudget::default())
    }

    fn key(k: &str) -> Seg {
        Seg::Key(k.to_string())
    }

    /// `label`, `inset`, and what activating it does — the whole shape of a page
    /// in one comparable form.
    fn shape(page: &Page) -> Vec<(String, usize, &'static str)> {
        page.items
            .iter()
            .map(|i| {
                let kind = match i.kind {
                    ItemKind::Scalar => "scalar",
                    ItemKind::Drill { .. } => "drill",
                    ItemKind::GroupHeader { .. } => "group",
                };
                (i.label.clone(), i.inset, kind)
            })
            .collect()
    }

    #[test]
    fn the_root_page_lists_one_level_and_drills_the_rest() {
        let root = sample();
        assert_eq!(
            shape(&page_of(&root, &[])),
            vec![
                ("title".into(), 0, "scalar"),
                ("version".into(), 0, "scalar"),
                ("enabled".into(), 0, "scalar"),
                // Mixed children (two containers among four) — a page of its own.
                ("server".into(), 0, "drill"),
            ]
        );
    }

    #[test]
    fn small_all_scalar_containers_inline_into_the_page() {
        let root = sample();
        // `tags` (2 strings) and `limits` (2 numbers) are both small and entirely
        // scalar, so `server` renders as one page rather than three.
        assert_eq!(
            shape(&page_of(&root, &[key("server")])),
            vec![
                ("host".into(), 0, "scalar"),
                ("port".into(), 0, "scalar"),
                ("tags".into(), 0, "group"),
                ("[0]".into(), 1, "scalar"),
                ("[1]".into(), 1, "scalar"),
                ("limits".into(), 0, "group"),
                ("max_connections".into(), 1, "scalar"),
                ("timeout".into(), 1, "scalar"),
            ]
        );
    }

    #[test]
    fn an_inlined_member_keeps_its_own_path() {
        let root = sample();
        let page = page_of(&root, &[key("server")]);
        let timeout = page
            .items
            .iter()
            .find(|i| i.label == "timeout")
            .expect("timeout on server's page");
        // The path is the document's, not the page's — an edit op takes it as-is
        // even though the row is two ranks below the page's focus.
        assert_eq!(
            timeout.path,
            vec![key("server"), key("limits"), key("timeout")]
        );
    }

    #[test]
    fn a_container_too_big_to_inline_drills() {
        let mut src = String::from("[big]\n");
        for i in 0..=INLINE_MAX {
            src.push_str(&format!("k{i} = {i}\n"));
        }
        let root = value_of(&src, Format::Toml);
        assert_eq!(
            shape(&page_of(&root, &[])),
            vec![("big".into(), 0, "drill")]
        );

        // One fewer child and the same container inlines.
        let trimmed = src
            .rsplit_once('\n')
            .unwrap()
            .0
            .rsplit_once('\n')
            .unwrap()
            .0;
        let root = value_of(&format!("{trimmed}\n"), Format::Toml);
        assert_eq!(page_of(&root, &[]).items[0].inset, 0);
        assert!(matches!(
            page_of(&root, &[]).items[0].kind,
            ItemKind::GroupHeader { .. }
        ));
    }

    #[test]
    fn a_container_holding_a_container_drills_however_small() {
        let root = value_of("{\"a\": {\"b\": {\"c\": 1}}}", Format::Json);
        // `a` has one child — but that child is a container, so inlining it would
        // put a group inside a group and reintroduce unbounded depth.
        assert_eq!(shape(&page_of(&root, &[])), vec![("a".into(), 0, "drill")]);
        assert_eq!(
            shape(&page_of(&root, &[key("a")])),
            vec![("b".into(), 0, "group"), ("c".into(), 1, "scalar")]
        );
    }

    // ── the page's own row limit ──────────────────────────────────────────

    /// A list of entries that each inline comfortably and are numerous enough
    /// that inlining all of them buries the page — the `repos.figl` shape.
    fn list_of(n: usize) -> Value {
        let items: Vec<String> = (0..n)
            .map(|i| format!(r#"{{"name": "r{i}", "lang": "rust"}}"#))
            .collect();
        value_of(
            &format!(r#"{{"repo": [{}]}}"#, items.join(", ")),
            Format::Json,
        )
    }

    #[test]
    fn a_list_long_enough_to_bury_the_page_is_listed_rather_than_expanded() {
        // Every item passes the per-item budget: two scalars, one rank. The
        // page still refuses them, because eight of them are twenty-four rows.
        let root = list_of(8);
        assert!(inlines(
            &value_of(r#"{"name": "r0", "lang": "rust"}"#, Format::Json),
            InlineBudget::default()
        ));
        let page = page_of(&root, &[key("repo")]);
        assert!(
            page.items.iter().all(PageItem::is_drill),
            "{:?}",
            shape(&page)
        );
        assert_eq!(page.items.len(), 8);
    }

    #[test]
    fn the_page_limit_is_asked_once_per_page_not_once_per_item() {
        let root = list_of(8);
        // Room for all twenty-four rows — a header and two fields apiece — and
        // the same list inlines. Nothing about the items changed, only what the
        // page can afford.
        let roomy = InlineBudget {
            page_rows: 24,
            ..InlineBudget::default()
        };
        let page = budgeted(&root, &[key("repo")], roomy);
        assert_eq!(page.items.iter().filter(|i| i.inset == 0).count(), 8);
        assert!(!page.has_drills(), "{:?}", shape(&page));

        // One row short of the total is a no, and it is a no for every item:
        // a list renders uniformly or not at all.
        let tight = InlineBudget {
            page_rows: 23,
            ..InlineBudget::default()
        };
        let page = budgeted(&root, &[key("repo")], tight);
        assert!(page.items.iter().all(PageItem::is_drill));
    }

    #[test]
    fn a_mapping_is_not_held_to_the_page_limit() {
        // Two groups of three, under a page limit that a sequence of the same
        // size would fail. A mapping's children are decided one at a time, so
        // holding them to a running total would inline whichever key came
        // first — a page whose shape depends on key order.
        let root = value_of(
            r#"{"a": {"x": 1, "y": 2, "z": 3}, "b": {"x": 1, "y": 2, "z": 3}}"#,
            Format::Json,
        );
        let page = budgeted(
            &root,
            &[],
            InlineBudget {
                page_rows: 4,
                ..InlineBudget::default()
            },
        );
        assert!(!page.has_drills(), "{:?}", shape(&page));
    }

    #[test]
    fn a_long_list_of_scalars_is_still_just_its_items() {
        // The page limit governs what a list *expands*; a sequence of scalars
        // has nothing to expand, and one row each is the only rendering there
        // is however many of them there are.
        let items: Vec<String> = (0..40).map(|i| i.to_string()).collect();
        let root = value_of(&format!("{{\"ns\": [{}]}}", items.join(", ")), Format::Json);
        let page = page_of(&root, &[key("ns")]);
        assert_eq!(page.items.len(), 40);
        assert!(page.items.iter().all(PageItem::is_scalar));
    }

    // ── fitting a budget to the room ──────────────────────────────────────

    #[test]
    fn a_document_that_fits_the_room_needs_no_navigation_at_all() {
        let root = sample();
        // Twelve rows of document. Given twelve rows of room, the root page is
        // the document and there is nothing left to open.
        let page = budgeted(&root, &[], InlineBudget::fitting(&root, 12));
        assert_eq!(page.items.len(), 12);
        assert!(!page.has_drills(), "{:?}", shape(&page));
    }

    #[test]
    fn a_document_one_row_too_big_falls_back_to_the_founding_rule() {
        let root = sample();
        let budget = InlineBudget::fitting(&root, 11);
        // Eleven rows of room is a short terminal, and a third of it is less
        // than the founding constant — so this *is* the founding rule, with
        // the page's own limit set to the room.
        assert_eq!(budget.rows, INLINE_MAX);
        assert_eq!(budget.page_rows, 11);
        assert!(budgeted(&root, &[], budget).has_drills());
    }

    #[test]
    fn a_document_that_does_not_fit_still_inlines_a_share_of_the_room() {
        // A group of eleven and a group of thirty, forty-three rows between
        // them: the founding rule drills both, and so does a short terminal.
        // A tall one that still cannot take the document whole has room to
        // show the eleven as the group it is, and still drills the thirty —
        // which would take most of the page, and a group that takes the page
        // is the page.
        let members: Vec<String> = (0..11).map(|i| format!("\"m{i}\"")).collect();
        let deps: Vec<String> = (0..30).map(|i| format!("\"d{i}\": {i}")).collect();
        let root = value_of(
            &format!(
                "{{\"members\": [{}], \"deps\": {{{}}}}}",
                members.join(", "),
                deps.join(", ")
            ),
            Format::Json,
        );
        let short = InlineBudget::fitting(&root, 20);
        assert_eq!(short.rows, INLINE_MAX);
        let page = budgeted(&root, &[], short);
        assert_eq!(
            shape(&page),
            [("members".into(), 0, "drill"), ("deps".into(), 0, "drill")]
        );

        let tall = InlineBudget::fitting(&root, 40);
        assert_eq!(tall.rows, 13);
        assert_eq!(tall.page_rows, 40);
        let page = budgeted(&root, &[], tall);
        let kinds = shape(&page);
        assert_eq!(kinds.len(), 1 + 11 + 1, "{kinds:?}");
        assert_eq!(kinds[0], ("members".into(), 0, "group"));
        assert_eq!(kinds[12], ("deps".into(), 0, "drill"));
    }

    #[test]
    fn a_document_too_deep_to_read_drills_however_short_it_is() {
        // Four rows and four ranks. It fits the room by the row count and not
        // by the eye: inlining it would draw eight columns of inset. Nor does
        // `a` inline as a group on the root page, which would draw the same
        // insets by another route: a group may nest one rank less than a
        // document, and `a` is three ranks deep.
        let root = value_of(r#"{"a": {"b": {"c": {"d": 1}}}}"#, Format::Json);
        let budget = InlineBudget::fitting(&root, 100);
        assert_eq!(budget.depth, FIT_MAX_DEPTH - 1);
        assert!(budgeted(&root, &[], budget).has_drills());
    }

    #[test]
    fn a_room_of_nothing_still_leaves_the_founding_rule_intact() {
        // A terminal too short to draw anything is not a reason to stop
        // inlining the small groups the founding rule was written for.
        let budget = InlineBudget::fitting(&sample(), 0);
        assert_eq!(budget.page_rows, INLINE_MAX);
        assert_eq!(
            shape(&budgeted(&sample(), &[key("server")], budget)),
            shape(&page_of(&sample(), &[key("server")]))
        );
    }

    #[test]
    fn raising_the_subtree_limit_raises_the_page_limit_with_it() {
        // A caller asking for a hundred rows of subtree is asking for a page
        // that can hold them.
        assert_eq!(InlineBudget::new(99, 8).page_rows, 99);
        // And one asking for less than a page's worth does not lower it.
        assert_eq!(InlineBudget::new(2, 1).page_rows, PAGE_INLINE_MAX);
    }

    #[test]
    fn a_page_of_one_row_is_a_label_rather_than_a_choice() {
        let root = value_of(
            r#"{"repo": [{"a": 1, "b": 2, "c": 3, "d": 4}]}"#,
            Format::Json,
        );
        assert!(!page_of(&root, &[]).has_choice());
        assert!(page_of(&root, &[key("repo"), Seg::Index(0)]).has_choice());
    }

    // ── the inline budget ─────────────────────────────────────────────────

    #[test]
    fn a_deeper_budget_inlines_a_nested_container_rank_by_rank() {
        let root = value_of("{\"a\": {\"b\": {\"c\": 1}}}", Format::Json);
        // `a`'s subtree reaches two ranks below its header — `b`, then `c`
        // under it — so a depth of 2 admits the whole chain onto the root page.
        let page = budgeted(&root, &[], InlineBudget::new(6, 2));
        assert_eq!(
            shape(&page),
            vec![
                ("a".into(), 0, "group"),
                ("b".into(), 1, "group"),
                ("c".into(), 2, "scalar"),
            ]
        );
        // One rank shy and it drills exactly as the default does.
        let page = budgeted(&root, &[], InlineBudget::new(6, 1));
        assert_eq!(shape(&page), vec![("a".into(), 0, "drill")]);
    }

    #[test]
    fn the_row_limit_counts_the_whole_subtree_headers_included() {
        let root = value_of(
            r#"{"outer": {"g": {"x": 1, "y": 2}, "z": 3}}"#,
            Format::Json,
        );
        // `outer` costs four rows: `g`'s header, its two members, and `z`.
        let fits = InlineBudget::new(4, 2);
        assert!(matches!(
            budgeted(&root, &[], fits).items[0].kind,
            ItemKind::GroupHeader { .. }
        ));
        let short = InlineBudget::new(3, 2);
        assert!(budgeted(&root, &[], short).items[0].is_drill());
    }

    #[test]
    fn a_generous_budget_puts_the_whole_document_on_the_root_page() {
        // The absorbed settings list: raise the budget past the document's size
        // and the root page simply is the document, ranks drawn as insets.
        let root = sample();
        let page = budgeted(&root, &[], InlineBudget::new(99, 8));
        assert_eq!(
            shape(&page),
            vec![
                ("title".into(), 0, "scalar"),
                ("version".into(), 0, "scalar"),
                ("enabled".into(), 0, "scalar"),
                ("server".into(), 0, "group"),
                ("host".into(), 1, "scalar"),
                ("port".into(), 1, "scalar"),
                ("tags".into(), 1, "group"),
                ("[0]".into(), 2, "scalar"),
                ("[1]".into(), 2, "scalar"),
                ("limits".into(), 1, "group"),
                ("max_connections".into(), 2, "scalar"),
                ("timeout".into(), 2, "scalar"),
            ]
        );
        assert!(!page.has_drills(), "nothing left to navigate to");
    }

    #[test]
    fn an_inlined_subtree_keeps_every_paths_own_address() {
        let root = sample();
        let page = budgeted(&root, &[], InlineBudget::new(99, 8));
        let timeout = page
            .items
            .iter()
            .find(|i| i.label == "timeout")
            .expect("timeout inlined onto the root page");
        assert_eq!(
            timeout.path,
            vec![key("server"), key("limits"), key("timeout")]
        );
    }

    #[test]
    fn a_budget_that_admits_a_chain_inlines_it_instead_of_compressing() {
        let root = value_of(LONE, Format::Json);
        let page = budgeted(&root, &[], InlineBudget::new(99, 8));
        let exports = &page.items[0];
        // Under the default budget this row compresses to `exports › journal`;
        // with room for the whole subtree there is no page to skip.
        assert!(matches!(exports.kind, ItemKind::GroupHeader { .. }));
        assert!(!exports.is_compressed());
    }

    #[test]
    fn a_sequence_of_nested_mappings_inlines_uniformly_under_a_deep_budget() {
        let root = value_of(STEPS, Format::Json);
        // The third step nests a `with` mapping, which the default budget's one
        // rank refuses — and uniformity then drills every item. Two ranks admit
        // it, so the whole list inlines, titles on the item headers.
        let page = budgeted(&root, &[key("steps")], InlineBudget::new(20, 2));
        let headers: Vec<_> = page
            .items
            .iter()
            .filter(|i| i.inset == 0)
            .map(|i| {
                (
                    i.title.clone(),
                    matches!(i.kind, ItemKind::GroupHeader { .. }),
                )
            })
            .collect();
        assert_eq!(headers.len(), 4);
        assert!(headers.iter().all(|(_, is_group)| *is_group));
        assert_eq!(headers[0].0.as_deref(), Some("actions/checkout@v7"));
        // The nested `with` renders as a group one rank further in.
        let with = page.items.iter().find(|i| i.label == "with").expect("with");
        assert_eq!(with.inset, 1);
        assert!(matches!(with.kind, ItemKind::GroupHeader { .. }));
    }

    #[test]
    fn demotion_still_folds_a_deeply_inlined_subtree_in_one_run() {
        let root = sample();
        let set: HashSet<String> = ["server".to_string()].into();
        let page = build_page(&root, &[], &HashSet::new(), &set, InlineBudget::new(99, 8));
        let (primary, advanced) = page.partitioned();
        assert_eq!(
            primary.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            ["title", "version", "enabled"]
        );
        // The whole inlined subtree is one contiguous demoted run — the fold
        // cannot cut a group in half however deep the budget let it nest.
        assert_eq!(advanced.len(), 9);
        assert!(advanced.iter().all(|i| i.demoted));
    }

    #[test]
    fn an_empty_container_drills_rather_than_inlining_as_a_headless_group() {
        let root = value_of("{\"empty\": {}, \"none\": []}", Format::Json);
        assert_eq!(
            shape(&page_of(&root, &[])),
            vec![("empty".into(), 0, "drill"), ("none".into(), 0, "drill")]
        );
        assert!(page_of(&root, &[key("empty")]).is_empty());
    }

    #[test]
    fn hiding_is_scoped_to_the_root_page() {
        let root = value_of(
            "{\"id\": 1, \"inner\": {\"id\": 2, \"keep\": 3}}",
            Format::Json,
        );
        let hidden = HashSet::from(["id".to_string()]);
        let rooted = build_page(
            &root,
            &[],
            &hidden,
            &HashSet::new(),
            InlineBudget::default(),
        );
        assert_eq!(
            shape(&rooted),
            vec![
                ("inner".into(), 0, "group"),
                ("id".into(), 1, "scalar"),
                ("keep".into(), 1, "scalar")
            ]
        );
        // The nested `id` shares the name and is untouched — the group inlined
        // into the root page still carries it.
        let inner = build_page(
            &root,
            &[key("inner")],
            &hidden,
            &HashSet::new(),
            InlineBudget::default(),
        );
        assert_eq!(
            shape(&inner),
            vec![("id".into(), 0, "scalar"), ("keep".into(), 0, "scalar")]
        );
    }

    #[test]
    fn a_page_that_cannot_be_listed_is_empty_rather_than_a_panic() {
        let root = sample();
        assert!(page_of(&root, &[key("nope")]).is_empty());
        assert!(page_of(&root, &[key("title")]).is_empty());
    }

    #[test]
    fn breadcrumbs_name_the_lineage() {
        let root = sample();
        assert_eq!(page_of(&root, &[]).breadcrumb("‹document›"), "‹document›");
        assert_eq!(
            page_of(&root, &[key("server"), key("limits")]).breadcrumb("‹document›"),
            "server › limits"
        );
        assert_eq!(
            page_of(&root, &[key("server"), key("tags")]).breadcrumb("x"),
            "server › tags"
        );
    }

    #[test]
    fn a_flat_document_has_nothing_to_drill_into() {
        let flat = value_of("{\"a\": 1, \"b\": 2}", Format::Json);
        assert!(!page_of(&flat, &[]).has_drills());
        assert!(page_of(&sample(), &[]).has_drills());
    }

    // ── titles for sequence items ─────────────────────────────────────────

    /// A workflow's steps: the case with no single naming key. Different kinds of
    /// step are named by different fields, and one field (`if`) reads the same on
    /// the items that have it.
    const STEPS: &str = r#"{"steps": [
        {"uses": "actions/checkout@v7"},
        {"uses": "dtolnay/rust-toolchain@stable", "if": "always"},
        {"uses": "Swatinem/rust-cache@v2", "if": "always", "with": {"key": "a"}},
        {"run": "cargo xtask ci", "shell": "bash"}
    ]}"#;

    fn titles(page: &Page) -> Vec<Option<String>> {
        page.items.iter().map(|i| i.title.clone()).collect()
    }

    #[test]
    fn a_sequence_item_is_titled_by_the_field_that_distinguishes_it() {
        let root = value_of(STEPS, Format::Json);
        let page = page_of(&root, &[key("steps")]);
        assert_eq!(
            titles(&page),
            vec![
                Some("actions/checkout@v7".into()),
                Some("dtolnay/rust-toolchain@stable".into()),
                Some("Swatinem/rust-cache@v2".into()),
                // No `uses` at all — falls to the next-best key it does have.
                Some("cargo xtask ci".into()),
            ]
        );
    }

    #[test]
    fn a_key_that_reads_the_same_on_every_item_loses_to_one_that_does_not() {
        let root = value_of(STEPS, Format::Json);
        let Value::Map(entries) = &root else {
            unreachable!()
        };
        let Value::Seq(items) = &entries[0].1 else {
            unreachable!()
        };
        let ranking = title_keys(items);
        // `if` is on two items and says "always" on both, so it names neither.
        let uses = ranking.iter().position(|k| k == "uses").expect("uses");
        let cond = ranking.iter().position(|k| k == "if").expect("if");
        assert!(uses < cond, "{ranking:?}");
        // `with` is a container: never a title.
        assert!(!ranking.iter().any(|k| k == "with"), "{ranking:?}");
    }

    #[test]
    fn a_conventional_name_key_outranks_a_merely_distinct_one() {
        let root = value_of(
            r#"{"env": [
                {"name": "HOME", "value": "/root"},
                {"name": "PATH", "value": "/bin"}
            ]}"#,
            Format::Json,
        );
        let page = page_of(&root, &[key("env")]);
        // Both items are small and all-scalar, so they inline — and the title
        // lands on the group header, which is the row standing in for the item.
        // `value` is exactly as distinct and as well covered as `name`; `name`
        // wins because it is what a config author means by a name.
        assert_eq!(
            page.items
                .iter()
                .filter(|i| i.inset == 0)
                .map(|i| i.title.clone())
                .collect::<Vec<_>>(),
            vec![Some("HOME".into()), Some("PATH".into())]
        );
    }

    #[test]
    fn a_mapping_entry_is_never_titled() {
        let root = sample();
        assert!(page_of(&root, &[]).items.iter().all(|i| i.title.is_none()));
        // Nor is a sequence of scalars: the value is already the whole row.
        let tags = page_of(&root, &[key("server"), key("tags")]);
        assert!(tags.items.iter().all(|i| i.title.is_none()));
    }

    #[test]
    fn a_sequence_renders_its_items_uniformly() {
        let root = value_of(STEPS, Format::Json);
        let page = page_of(&root, &[key("steps")]);
        // The third step nests a `with` mapping, so it cannot inline — and none of
        // the others do either, however small. A list reads as a list.
        assert!(
            page.items
                .iter()
                .all(|i| matches!(i.kind, ItemKind::Drill { .. })),
            "{:?}",
            shape(&page)
        );

        // Take the nesting away and every item inlines, again as a group.
        let flat = value_of(
            r#"{"steps": [{"run": "a"}, {"run": "b", "shell": "sh"}]}"#,
            Format::Json,
        );
        let page = page_of(&flat, &[key("steps")]);
        assert_eq!(
            shape(&page),
            vec![
                ("[0]".into(), 0, "group"),
                ("run".into(), 1, "scalar"),
                ("[1]".into(), 0, "group"),
                ("run".into(), 1, "scalar"),
                ("shell".into(), 1, "scalar"),
            ]
        );
    }

    #[test]
    fn a_titled_item_carries_its_title_into_its_own_breadcrumb() {
        let root = value_of(STEPS, Format::Json);
        let page = page_of(&root, &[key("steps"), Seg::Index(3)]);
        assert_eq!(page.title.as_deref(), Some("cargo xtask ci"));
        assert_eq!(page.breadcrumb("‹document›"), "steps › cargo xtask ci");
        // Its own children are mapping entries, so none of them is titled.
        assert!(page.items.iter().all(|i| i.title.is_none()));
    }

    #[test]
    fn a_multi_line_value_is_cut_to_its_first_line() {
        let root = value_of(
            "{\"steps\": [{\"run\": \"set -e\\ncargo test\\n\"}]}",
            Format::Json,
        );
        let page = page_of(&root, &[key("steps")]);
        // A YAML block scalar would otherwise draw a row several lines tall and
        // throw every row below it out of alignment.
        assert_eq!(page.items[0].title.as_deref(), Some("set -e …"));
        assert!(!page.items[0].preview.contains('\n'));
    }

    // ── flow summaries ────────────────────────────────────────────────────

    #[test]
    fn a_container_that_fits_on_the_row_shows_its_contents_not_a_count() {
        let root = value_of(
            r#"{"on": {"push": {"branches": ["main"]}, "pull_request": null}}"#,
            Format::Json,
        );
        let page = page_of(&root, &[key("on")]);
        let push = &page.items[0];
        // It has one field, and the field is right there: counting it to `1 field`
        // would say strictly less than the document does in the same room.
        assert!(matches!(push.kind, ItemKind::Drill { count: 1 }));
        assert_eq!(push.summary.as_deref(), Some("{branches: [main]}"));
    }

    #[test]
    fn a_container_too_long_to_summarise_falls_back_to_being_counted() {
        let long = "x".repeat(SUMMARY_BUDGET);
        let root = value_of(
            &format!(r#"{{"outer": {{"a": {{"b": "{long}"}}}}}}"#),
            Format::Json,
        );
        let page = page_of(&root, &[key("outer")]);
        assert!(page.items[0].summary.is_none());
        // The budget is a length limit, not a depth one — shorten the value and
        // the same shape summarises fine.
        let root = value_of(r#"{"outer": {"a": {"b": "x"}}}"#, Format::Json);
        assert_eq!(
            page_of(&root, &[key("outer")]).items[0].summary.as_deref(),
            Some("{b: x}")
        );
    }

    #[test]
    fn a_titled_row_does_not_spend_its_width_saying_its_title_twice() {
        let root = list_of(8);
        let page = page_of(&root, &[key("repo")]);
        let first = &page.items[0];
        // The row already reads `[0] · r0`; a summary opening `name: r0` would
        // say it again in the same line.
        assert_eq!(first.title.as_deref(), Some("r0"));
        assert_eq!(first.summary.as_deref(), Some("{lang: rust}"));
        // Elided from the summary, not from the document: the count still
        // counts it, and it is on the page the row opens.
        assert!(matches!(first.kind, ItemKind::Drill { count: 2 }));
        assert_eq!(
            page_of(&root, &[key("repo"), Seg::Index(0)])
                .items
                .iter()
                .map(|i| i.label.as_str())
                .collect::<Vec<_>>(),
            ["name", "lang"]
        );
    }

    #[test]
    fn a_row_whose_only_field_is_its_title_falls_back_to_being_counted() {
        // Nothing left once the title is elided, and `{}` would be a lie about
        // a mapping that has a field in it. The count says the rest.
        let items: Vec<String> = (0..8).map(|i| format!(r#"{{"name": "r{i}"}}"#)).collect();
        let root = value_of(
            &format!(r#"{{"repo": [{}]}}"#, items.join(", ")),
            Format::Json,
        );
        let page = budgeted(
            &root,
            &[key("repo")],
            InlineBudget {
                page_rows: 4,
                ..InlineBudget::default()
            },
        );
        assert_eq!(page.items[0].title.as_deref(), Some("r0"));
        assert!(page.items[0].summary.is_none());
        assert!(matches!(page.items[0].kind, ItemKind::Drill { count: 1 }));
    }

    #[test]
    fn compression_and_a_title_cannot_meet() {
        // What lets the elision summarise `child` without checking whether the
        // row compressed past it. Compression needs a container holding one
        // container and nothing else; a title needs a scalar field. A row can
        // have either and never both — so a compressed row's summary is of the
        // node its title would have come from, vacuously.
        let root = value_of(
            r#"{"repo": [{"only": {"name": "inner", "lang": "rust", "a": 1,
                                   "b": 2, "c": 3, "d": 4, "e": 5}}]}"#,
            Format::Json,
        );
        let row = &page_of(&root, &[key("repo")]).items[0];
        assert!(row.is_compressed(), "{:?}", row.chain_labels());
        assert_eq!(row.chain_labels(), ["[0]", "only"]);
        // Nothing elided, because there was no title to elide.
        assert!(row.title.is_none());
        assert!(
            row.summary
                .as_deref()
                .is_some_and(|f| f.starts_with("{name: inner")),
            "{:?}",
            row.summary
        );
    }

    #[test]
    fn a_mapping_entry_summarises_whole_because_nothing_titled_it() {
        // Only a sequence item takes a title, so only a sequence item has one
        // to elide. A `name` under a key is just a field.
        let root = value_of(r#"{"a": {"name": "x", "lang": "rust"}}"#, Format::Json);
        let page = budgeted(
            &root,
            &[],
            InlineBudget {
                rows: 1,
                ..InlineBudget::default()
            },
        );
        assert_eq!(
            page.items[0].summary.as_deref(),
            Some("{name: x, lang: rust}")
        );
    }

    #[test]
    fn a_scalar_is_never_summarised() {
        let root = sample();
        let page = page_of(&root, &[]);
        assert!(
            page.items
                .iter()
                .filter(|i| matches!(i.kind, ItemKind::Scalar))
                .all(|i| i.summary.is_none())
        );
    }

    #[test]
    fn a_group_header_is_not_a_drill() {
        let root = sample();
        let page = page_of(&root, &[key("server")]);
        let limits = page
            .items
            .iter()
            .find(|i| i.label == "limits")
            .expect("limits");
        assert!(matches!(limits.kind, ItemKind::GroupHeader { .. }));
        // It names a container, but opening it would show what is already here.
        assert!(limits.is_container());
        assert!(!limits.is_drill());
        assert!(!page.has_drills());
    }

    #[test]
    fn a_demoted_key_is_marked_but_still_listed_in_document_order() {
        let root = sample();
        let page = demoting(&root, &[], &["version"]);
        // Demotion is not hiding: the row is still there, still where the
        // document put it. Only `demoted` moved.
        assert_eq!(
            shape(&page)
                .iter()
                .map(|(l, _, _)| l.as_str())
                .collect::<Vec<_>>(),
            ["title", "version", "enabled", "server"]
        );
        let demoted: Vec<&str> = page
            .items
            .iter()
            .filter(|i| i.demoted)
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(demoted, ["version"]);
    }

    #[test]
    fn partitioning_folds_the_demoted_run_to_the_end_and_keeps_both_orders() {
        let root = sample();
        let page = demoting(&root, &[], &["title", "server"]);
        let (primary, advanced) = page.partitioned();
        assert_eq!(
            primary.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            ["version", "enabled"]
        );
        assert_eq!(
            advanced
                .iter()
                .map(|i| i.label.as_str())
                .collect::<Vec<_>>(),
            ["title", "server"]
        );
    }

    #[test]
    fn demotion_covers_the_whole_subtree_so_drilling_in_stays_demoted() {
        let root = sample();
        // The row on the root's page.
        let at_root = demoting(&root, &[], &["server"]);
        assert!(
            at_root
                .items
                .iter()
                .find(|i| i.label == "server")
                .unwrap()
                .demoted
        );

        // The page that row opens, and everything on it — including the members
        // of a group inlined into it, which are two segments deeper still.
        let inside = demoting(&root, &[key("server")], &["server"]);
        assert!(inside.demoted);
        assert!(inside.items.iter().all(|i| i.demoted));
        assert!(inside.items.iter().any(|i| i.inset == 1));

        let deeper = demoting(&root, &[key("server"), key("limits")], &["server"]);
        assert!(deeper.demoted);
        assert!(deeper.items.iter().all(|i| i.demoted));
    }

    #[test]
    fn demotion_is_root_scoped_so_a_nested_key_of_the_same_name_is_untouched() {
        // `host` is demoted at the root; the `host` *inside* `server` is a
        // different field that happens to share a spelling.
        let root = value_of(
            r#"{"host": "managed", "server": {"host": "localhost", "port": 8080}}"#,
            Format::Json,
        );
        let page = demoting(&root, &[], &["host"]);
        assert!(
            page.items
                .iter()
                .find(|i| i.label == "host")
                .unwrap()
                .demoted
        );
        assert!(
            !page
                .items
                .iter()
                .find(|i| i.label == "server")
                .unwrap()
                .demoted
        );

        let inside = demoting(&root, &[key("server")], &["host"]);
        assert!(!inside.demoted);
        assert!(inside.items.iter().all(|i| !i.demoted));
    }

    #[test]
    fn a_group_header_and_its_inlined_members_never_land_on_opposite_sides() {
        let root = sample();
        let page = demoting(&root, &[key("server")], &["server"]);
        let (primary, advanced) = page.partitioned();
        // Both the header and the members inlined under it are in the same run,
        // so the fold cannot cut the group in half.
        assert!(primary.is_empty());
        let labels: Vec<&str> = advanced.iter().map(|i| i.label.as_str()).collect();
        let header = labels.iter().position(|l| *l == "limits").expect("limits");
        assert_eq!(&labels[header..], ["limits", "max_connections", "timeout"]);
    }

    /// The shape that prompted compression: a category holding one export, which
    /// holds a map, so it cannot inline and earns a page with one row on it.
    const LONE: &str = r#"{
      "exports": {"journal": {"label": "Public Journal",
                              "gate": {"field": "audience", "value": "public"}}},
      "diaryx": {"publish": {"audiences": [{"name": "public", "gates": []}]}},
      "plain": {"a": 1, "b": 2}
    }"#;

    #[test]
    fn a_row_whose_page_would_hold_only_it_names_the_chain_instead() {
        let root = value_of(LONE, Format::Json);
        let page = page_of(&root, &[]);
        let exports = &page.items[0];

        assert!(exports.is_compressed());
        assert_eq!(exports.chain_labels(), ["exports", "journal"]);
        // Described by what it lands on: `journal`'s two fields, not `exports`'
        // one.
        assert!(matches!(exports.kind, ItemKind::Drill { count: 2 }));
        assert_eq!(exports.descend_to, vec![key("exports"), key("journal")]);
        // The path is still the outermost node, so every op takes it unchanged.
        assert_eq!(exports.path, vec![key("exports")]);
    }

    #[test]
    fn compression_follows_the_chain_as_far_as_it_goes_including_a_lone_seq_item() {
        let root = value_of(LONE, Format::Json);
        let diaryx = &page_of(&root, &[])
            .items
            .iter()
            .find(|i| i.label == "diaryx")
            .expect("diaryx")
            .clone();
        // diaryx → publish → audiences → [0], and only then a page with two
        // things on it.
        assert_eq!(
            diaryx.chain_labels(),
            ["diaryx", "publish", "audiences", "[0]"]
        );
        assert!(matches!(diaryx.kind, ItemKind::Drill { count: 2 }));
    }

    #[test]
    fn a_page_with_something_to_say_is_never_compressed_past() {
        let root = value_of(LONE, Format::Json);
        let page = page_of(&root, &[]);
        let plain = page.items.iter().find(|i| i.label == "plain").unwrap();
        // `plain` holds two scalars, so it inlines — nothing to compress, and
        // the group header is not a drill at all.
        assert!(!plain.is_compressed());
        assert_eq!(plain.chain_labels(), ["plain"]);
        assert_eq!(plain.descend_to, plain.path);

        // A lone *scalar* child is a real answer too: its page shows a value.
        let root = value_of(r#"{"outer": {"only": 1}}"#, Format::Json);
        let outer = &page_of(&root, &[]).items[0];
        assert!(!outer.is_compressed());
    }

    #[test]
    fn an_uncompressed_row_descends_to_where_it_already_points() {
        let root = sample();
        for focus in [vec![], vec![key("server")]] {
            for item in &page_of(&root, &focus).items {
                assert_eq!(item.descend_to, item.path, "{}", item.label);
                assert_eq!(item.chain_labels(), std::slice::from_ref(&item.label));
            }
        }
    }

    #[test]
    fn only_a_mapping_entry_can_be_renamed() {
        let root = sample();
        let page = page_of(&root, &[key("server"), key("tags")]);
        // A sequence item's label is its index — a position, not a name.
        assert!(page.items.iter().all(|i| !i.can_rename()));
        assert!(page_of(&root, &[]).items.iter().all(|i| i.can_rename()));
    }
}
