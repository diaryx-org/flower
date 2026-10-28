//! Laying a page out as a settings screen: which panes there are, which
//! sections each page falls into, what each row is called and says, and —
//! because Blitz cannot be asked how tall anything came out — how tall every
//! row is and where it sits, so a pane can scroll to the cursor and the input
//! method can be told where the caret is. No Dioxus type is named here, so
//! all of it is tested without a window.
//!
//! This is the seam the Swift host's `pageLayout` was, and its rules, in the
//! same words. The projection ships a flat list of items with insets and
//! group headers; a settings screen draws structure:
//!
//! - a **rank-0 group** is a section of its own, its name the caption above
//!   it and its members rebased a rank left;
//! - a **scalar sequence** is one row of chips wherever it sits, because a
//!   list of tags is one fact about the document, not `n` rows of positions;
//! - everything else stays a row at its inset, a nested group a caption
//!   inside its section;
//! - runs of rank-0 rows between titled sections collect into untitled
//!   sections, in document order; and
//! - the **demoted** rows fold behind an "Advanced" disclosure below the rest,
//!   where a page has both kinds.
//!
//! Heights are the stylesheet's (`view.rs`), which fixes every row's lines
//! rather than letting them wrap: the arithmetic and the stylesheet are two
//! spellings of one set of numbers, and the tests here hold the first.

use std::borrow::Cow;
use std::collections::HashSet;

use flower_core::{Backend, ItemKind, Model, Page, PageItem, Seg, VKind};

/// What a pane stands for, relative to the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The page the cursor is on.
    Current,
    /// The page one level out — the left pane, while the cursor's page is on
    /// the right.
    Parent,
    /// The page the cursor would open — the right pane, while the cursor's
    /// page leads the split and the right one is a preview.
    Peek,
}

/// A page in a pane. The cursor is drawn only on the current page; the parent
/// keeps the row the current page was opened through marked instead, and a
/// preview marks nothing.
pub struct Shown<'m> {
    pub role: Role,
    pub page: Cow<'m, Page>,
    pub selected: Option<usize>,
}

/// The panes the editor is cut into, left to right. `None` is the placeholder
/// pane: the right half while the cursor stands on something with nothing to
/// preview.
///
/// Two when there is room for two *and* something for the second to hold —
/// [`Model::pages_would_degenerate`] — and one otherwise. The two are
/// consecutive levels of one lineage: the page the cursor's was opened from
/// and the cursor's, or, while the cursor's page leads the split, the
/// cursor's and a preview of what it would open.
pub fn panes<B: Backend>(model: &Model<B>, two: bool) -> Vec<Option<Shown<'_>>> {
    let current = || Shown {
        role: Role::Current,
        page: Cow::Borrowed(model.page()),
        selected: Some(model.page_selected()),
    };
    if !two || model.pages_would_degenerate() {
        return vec![Some(current())];
    }
    if model.page_leads_the_split() {
        let peek = model.peek_page().map(|page| Shown {
            role: Role::Peek,
            page: Cow::Owned(page),
            selected: None,
        });
        return vec![Some(current()), peek];
    }
    let parent = model.parent_page();
    vec![
        Some(Shown {
            role: Role::Parent,
            page: Cow::Borrowed(parent),
            selected: parent.position_of(model.focus()),
        }),
        Some(current()),
    ]
}

// ── the sizes the stylesheet draws with ─────────────────────────────────────

/// Every size a page is drawn with, in CSS pixels, at a zoom. `view.rs`
/// writes them into the stylesheet and [`place`] adds them up, so the two
/// cannot disagree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
    /// The text's size: unit's `--unit-font-size`.
    pub font: f64,
    /// A pane's padding, all round.
    pub pane_pad: f64,
    /// The widest a pane's column of sections grows; wider panes centre it.
    pub column_max: f64,
    /// The gap between one section, the add row or the fold and the next.
    pub gap: f64,
    /// A section's caption, and the gap under it.
    pub caption: f64,
    pub caption_gap: f64,
    /// A row of one line: its title's line and its padding.
    pub row: f64,
    /// Each line a row has under its title: a finding, a refusal, a comment.
    pub subline: f64,
    /// The hairline between two rows of a section.
    pub divider: f64,
    /// A nested group's caption inside a section.
    pub group_row: f64,
    /// The pane's own title, over a pane that is not the cursor's.
    pub pane_title: f64,
    /// The add row and the fold's row.
    pub action_row: f64,
    /// How far one rank of inset sets a row in.
    pub indent: f64,
    /// An average character of the rows' face, for judging whether a list's
    /// chips fit beside its name. Generous: a chips row that does not fit is
    /// clipped, and a row per member never is.
    pub char: f64,
    /// The breadcrumb's bar along the top.
    pub bar: f64,
}

impl Sizes {
    /// At zoom 1: unit's regular density, a Mac's sizes.
    pub const BASE: Sizes = Sizes {
        font: 13.0,
        pane_pad: 14.0,
        column_max: 560.0,
        gap: 14.0,
        caption: 16.0,
        caption_gap: 6.0,
        row: 34.0,
        subline: 16.0,
        divider: 1.0,
        group_row: 28.0,
        pane_title: 16.0,
        action_row: 28.0,
        indent: 16.0,
        char: 7.5,
        bar: 38.0,
    };

    /// Every size times `zoom`, but the hairline, which stays a hairline.
    pub fn at(zoom: f64) -> Sizes {
        let b = Self::BASE;
        let z = |v: f64| (v * zoom).round();
        Sizes {
            font: z(b.font),
            pane_pad: z(b.pane_pad),
            column_max: z(b.column_max),
            gap: z(b.gap),
            caption: z(b.caption),
            caption_gap: z(b.caption_gap),
            row: z(b.row),
            subline: z(b.subline),
            divider: b.divider,
            group_row: z(b.group_row),
            pane_title: z(b.pane_title),
            action_row: z(b.action_row),
            indent: z(b.indent),
            char: b.char * zoom,
            bar: z(b.bar),
        }
    }
}

impl Default for Sizes {
    fn default() -> Self {
        Self::BASE
    }
}

// ── sections ────────────────────────────────────────────────────────────────

/// One drawable entry of a section.
#[derive(Debug)]
pub enum Entry<'p> {
    /// One item as one row, `inset` ranks in as drawn (the layout's rebased
    /// rank, not the projection's).
    Row {
        index: usize,
        item: &'p PageItem,
        inset: usize,
    },
    /// A group inlined inside a section: a caption over its members, which
    /// follow it as entries of their own.
    Group {
        index: usize,
        item: &'p PageItem,
        inset: usize,
    },
    /// A scalar sequence as one row: the header names it, the members are the
    /// chips. Every member is still its own item, which the cursor walks.
    Chips {
        index: usize,
        header: &'p PageItem,
        members: Vec<(usize, &'p PageItem)>,
        inset: usize,
    },
}

impl<'p> Entry<'p> {
    /// The item the entry is: a row's own, a group's or a chips row's header.
    pub fn index(&self) -> usize {
        match self {
            Entry::Row { index, .. } | Entry::Group { index, .. } | Entry::Chips { index, .. } => {
                *index
            }
        }
    }

    pub fn item(&self) -> &'p PageItem {
        match self {
            Entry::Row { item, .. } | Entry::Group { item, .. } => item,
            Entry::Chips { header, .. } => header,
        }
    }

    pub fn inset(&self) -> usize {
        match self {
            Entry::Row { inset, .. } | Entry::Group { inset, .. } | Entry::Chips { inset, .. } => {
                *inset
            }
        }
    }

    /// Whether the item at `selected` is drawn on this entry.
    pub fn holds(&self, selected: usize) -> bool {
        match self {
            Entry::Chips { index, members, .. } => {
                *index == selected || members.iter().any(|(i, _)| *i == selected)
            }
            e => e.index() == selected,
        }
    }
}

/// A run of a page's entries under one caption: a rank-0 group whose members
/// these are, or none for a run of the page's own rows.
#[derive(Debug)]
pub struct Section<'p> {
    pub header: Option<(usize, &'p PageItem)>,
    pub entries: Vec<Entry<'p>>,
}

/// A page laid out: the sections a reader came to edit, and, behind the fold,
/// the demoted ones.
#[derive(Debug, Default)]
pub struct Laid<'p> {
    pub sections: Vec<Section<'p>>,
    /// The fold's sections: empty when the page has no fold.
    pub advanced: Vec<Section<'p>>,
    /// How many rows the fold holds, which its row says while it is shut.
    pub folded: usize,
}

/// `page` as a settings screen in a column `width` CSS pixels wide.
///
/// The fold only exists where there are both kinds of row. A page of nothing
/// but demoted rows has nothing to protect them from, and a page *under* a
/// demoted key was opened on purpose (core marks the whole page, so every run
/// would be the folded one and the page would arrive shut).
pub fn lay_out<'p>(page: &'p Page, width: f64, z: &Sizes) -> Laid<'p> {
    let all: Vec<(usize, &PageItem)> = page.items.iter().enumerate().collect();
    let (promoted, demoted): (Vec<_>, Vec<_>) = all.iter().partition(|(_, i)| !i.demoted);
    if page.demoted || promoted.is_empty() || demoted.is_empty() {
        return Laid {
            sections: sections(&all, width, z),
            ..Laid::default()
        };
    }
    Laid {
        sections: sections(&promoted, width, z),
        advanced: sections(&demoted, width, z),
        folded: demoted.len(),
    }
}

/// A run of a page's items as sections.
fn sections<'p>(items: &[(usize, &'p PageItem)], width: f64, z: &Sizes) -> Vec<Section<'p>> {
    let mut out = Vec::new();
    let mut run: Vec<(usize, &'p PageItem)> = Vec::new();
    let flush = |run: &mut Vec<(usize, &'p PageItem)>, out: &mut Vec<Section<'p>>| {
        if !run.is_empty() {
            out.push(Section {
                header: None,
                entries: entries(run, 0, width, z),
            });
            run.clear();
        }
    };
    let mut i = 0;
    while i < items.len() {
        let (index, item) = items[i];
        if matches!(item.kind, ItemKind::GroupHeader { .. }) && item.inset == 0 {
            let mut j = i + 1;
            while j < items.len() && items[j].1.inset > 0 {
                j += 1;
            }
            let members = &items[i + 1..j];
            if members.is_empty() || chips_fit(item, members, 0, width, z) {
                // An empty group is a value, and a chips row is a row: both
                // stay in the surrounding section.
                run.extend_from_slice(&items[i..j]);
            } else {
                flush(&mut run, &mut out);
                out.push(Section {
                    header: Some((index, item)),
                    entries: entries(members, 1, width, z),
                });
            }
            i = j;
        } else {
            run.push((index, item));
            i += 1;
        }
    }
    flush(&mut run, &mut out);
    out
}

/// One section's entries: chips folded, every drawn inset shifted left by
/// `shift` (1 inside a titled section, whose caption already says what the
/// members belong to).
fn entries<'p>(
    items: &[(usize, &'p PageItem)],
    shift: usize,
    width: f64,
    z: &Sizes,
) -> Vec<Entry<'p>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let (index, item) = items[i];
        let inset = item.inset.saturating_sub(shift);
        if matches!(item.kind, ItemKind::GroupHeader { .. }) {
            let mut j = i + 1;
            while j < items.len() && items[j].1.inset > item.inset {
                j += 1;
            }
            let members = &items[i + 1..j];
            if !members.is_empty() {
                if chips_fit(item, members, inset, width, z) {
                    out.push(Entry::Chips {
                        index,
                        header: item,
                        members: members.to_vec(),
                        inset,
                    });
                    i = j;
                } else {
                    out.push(Entry::Group { index, item, inset });
                    i += 1;
                }
                continue;
            }
        }
        out.push(Entry::Row { index, item, inset });
        i += 1;
    }
    out
}

/// Whether a group is drawn as one row of chips: a scalar sequence, all of
/// whose members sit directly under it, short enough to share the row with
/// its name.
fn chips_fit(
    header: &PageItem,
    members: &[(usize, &PageItem)],
    inset: usize,
    width: f64,
    z: &Sizes,
) -> bool {
    if header.vkind != VKind::Seq
        || members.is_empty()
        || !members
            .iter()
            .all(|(_, m)| m.is_scalar() && m.inset == header.inset + 1)
    {
        return false;
    }
    let chips: f64 = members
        .iter()
        .map(|(_, m)| chip_width(&m.preview, z))
        .sum::<f64>()
        + 6.0 * members.len() as f64;
    let name = text_width(&row_name(header, false), z);
    chips + name + 2.0 * z.pane_pad + inset as f64 * z.indent + 4.0 * z.font <= width
}

/// A chip's width: its text, its padding, its border.
pub fn chip_width(text: &str, z: &Sizes) -> f64 {
    text_width(if text.is_empty() { "—" } else { text }, z) + 22.0
}

fn text_width(text: &str, z: &Sizes) -> f64 {
    text.chars().count() as f64 * z.char
}

// ── what a row says ─────────────────────────────────────────────────────────

/// The words of a key: split at `_`, `-` and `.`, and at every camelCase hump
/// — `noEmit` → `no`, `Emit`; `HTTPServer` → `HTTP`, `Server`. Case is kept.
pub fn key_words(key: &str) -> Vec<String> {
    let mut words = Vec::new();
    for part in key.split(['_', '-', '.']).filter(|p| !p.is_empty()) {
        let chars: Vec<char> = part.chars().collect();
        let mut word = String::new();
        for (i, &c) in chars.iter().enumerate() {
            if i > 0 && c.is_uppercase() {
                let prev = chars[i - 1];
                let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
                // `devD…` starts a word; so does the `S` of `HTTPServer`, the
                // last capital of a run that a lowercase letter follows.
                if prev.is_lowercase() || prev.is_numeric() || (prev.is_uppercase() && next_lower) {
                    words.push(std::mem::take(&mut word));
                }
            }
            word.push(c);
        }
        if !word.is_empty() {
            words.push(word);
        }
    }
    words
}

/// A key as a settings screen names it: `max_connections` → `Max
/// Connections`. Only a key shaped like a field name — letters, digits, and
/// the `_` `-` `.` between its words — is prettified; anything else is a name
/// somebody wrote, shown as written.
pub fn prettify(key: &str) -> String {
    let shaped = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if !shaped {
        return key.to_string();
    }
    key_words(key)
        .iter()
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().chain(cs).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What a row is called: the key title-cased, or the key as it is. The second
/// is not a fallback so much as a rule: a key that cannot be renamed is a
/// sequence index, and a `verbatim` key is a name somebody chose (see
/// [`verbatim`]) — either way, prettifying it would name the row something
/// the document does not contain.
pub fn row_name(item: &PageItem, verbatim: bool) -> String {
    if item.can_rename() && !verbatim {
        item.chain_labels()
            .iter()
            .map(|l| prettify(l))
            .collect::<Vec<_>>()
            .join(" › ")
    } else {
        item.chain_labels().join(" › ")
    }
}

/// What names a group or a section: the prettified key, and — for a titled
/// sequence item — the title that names it.
pub fn group_name(item: &PageItem) -> String {
    let own = row_name(item, false);
    match &item.title {
        Some(t) => format!("{own} · {t}"),
        None => own,
    }
}

/// What a container's row says about it: its contents when they fit, and a
/// count when they don't.
pub fn drill_summary(item: &PageItem) -> String {
    if let Some(s) = &item.summary {
        return s.clone();
    }
    let n = match item.kind {
        ItemKind::Drill { count } | ItemKind::GroupHeader { count } => count,
        ItemKind::Scalar => 0,
    };
    match (item.vkind == VKind::Seq, n) {
        (true, 1) => "1 item".into(),
        (true, n) => format!("{n} items"),
        (false, 1) => "1 field".into(),
        (false, n) => format!("{n} fields"),
    }
}

/// The rows of a page whose keys are data rather than field names, by index.
///
/// No schema says which maps are records and which are dictionaries, so this
/// reads it off the document: a nested map holding nothing but plain strings
/// — `scripts`, `dependencies`, `env`, an alias table — is keyed by names
/// somebody chose. You type `bun run dev`, not `bun run Dev`. The page's own
/// container is never one, nor a list's item: each is a record.
pub fn verbatim(page: &Page) -> HashSet<usize> {
    let parents = parents(page);
    let mut children: std::collections::HashMap<Option<usize>, Vec<usize>> = Default::default();
    for (i, p) in parents.iter().enumerate() {
        children.entry(*p).or_default().push(i);
    }
    let mut out = HashSet::new();
    for (parent, members) in children {
        // A list's item is a record — a step, a job — whatever its fields
        // hold, and its siblings name the same fields: prettified in one and
        // not the next, they would read as different fields.
        let Some(p) = parent else { continue };
        if matches!(page.items[p].path.last(), Some(Seg::Index(_))) {
            continue;
        }
        let dictionary = members.iter().all(|&i| {
            let it = &page.items[i];
            it.is_scalar() && it.vkind == VKind::Str && it.can_rename()
        });
        if dictionary {
            out.extend(members);
        }
    }
    out
}

/// Each item's parent row, in item order: the nearest earlier item one inset
/// shallower, or `None` for the page's own top-level rows. The projection
/// ships a flat list, and this is the containment it flattened.
pub fn parents(page: &Page) -> Vec<Option<usize>> {
    let mut last_at: Vec<usize> = Vec::new();
    page.items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let d = item.inset;
            let parent = if d == 0 {
                None
            } else {
                last_at.get(d - 1).copied()
            };
            last_at.truncate(d);
            last_at.push(i);
            parent
        })
        .collect()
}

/// Whether the item at `index` can move one place `by` among its siblings.
pub fn can_move(page: &Page, index: usize, by: isize) -> bool {
    let parents = parents(page);
    let Some(&p) = parents.get(index) else {
        return false;
    };
    let siblings: Vec<usize> = (0..page.items.len()).filter(|&i| parents[i] == p).collect();
    let Some(at) = siblings.iter().position(|&i| i == index) else {
        return false;
    };
    let to = at as isize + by;
    to >= 0 && (to as usize) < siblings.len()
}

/// The path a page's own container is reached by, as a breadcrumb lists it:
/// each step's path and what it is called, outermost first.
pub fn crumbs(page: &Page) -> Vec<(Vec<Seg>, String)> {
    let mut out = Vec::new();
    for n in 1..=page.focus.len() {
        let path = page.focus[..n].to_vec();
        let label = flower_core::page::seg_label(&page.focus[n - 1]);
        let named = match &page.focus[n - 1] {
            Seg::Key(_) => prettify(&label),
            Seg::Index(_) if n == page.focus.len() => page.title.clone().unwrap_or(label),
            Seg::Index(_) => label,
        };
        out.push((path, named));
    }
    out
}

// ── how tall everything is ──────────────────────────────────────────────────

/// A row's height with `sub` lines under its title.
pub fn row_height(sub: usize, z: &Sizes) -> f64 {
    z.row + sub as f64 * z.subline
}

/// One entry's height, with the lines `sub` says each row has.
pub fn entry_height(entry: &Entry<'_>, sub: &dyn Fn(&PageItem) -> usize, z: &Sizes) -> f64 {
    match entry {
        Entry::Group { .. } => z.group_row,
        e => row_height(sub(e.item()), z),
    }
}

/// Where each item of a laid-out page sits in its pane's content, top to
/// bottom: its index, its top and its height. And the content's height.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placed {
    pub rows: Vec<(usize, f64, f64)>,
    pub height: f64,
}

impl Placed {
    /// The box of the entry drawing item `index`: its top and its height.
    pub fn of(&self, index: usize) -> Option<(f64, f64)> {
        self.rows
            .iter()
            .find(|(i, _, _)| *i == index)
            .map(|&(_, y, h)| (y, h))
    }
}

/// Where everything on `laid` sits, as `view.rs` draws it: a pane title when
/// `titled`, the sections, the add row when `adds`, then the fold's row and,
/// when `open`, the fold's sections.
pub fn place(
    laid: &Laid<'_>,
    titled: bool,
    adds: bool,
    open: bool,
    sub: &dyn Fn(&PageItem) -> usize,
    z: &Sizes,
) -> Placed {
    let mut placed = Placed::default();
    let mut y = z.pane_pad;
    let mut first = true;
    let mut gap = |y: &mut f64| {
        if !std::mem::take(&mut first) {
            *y += z.gap;
        }
    };
    if titled {
        gap(&mut y);
        y += z.pane_title;
    }
    let section = |s: &Section<'_>, y: &mut f64, placed: &mut Placed| {
        if let Some((index, _)) = s.header {
            placed.rows.push((index, *y, z.caption));
            *y += z.caption + z.caption_gap;
        }
        for (n, e) in s.entries.iter().enumerate() {
            if n > 0 {
                *y += z.divider;
            }
            let h = entry_height(e, sub, z);
            placed.rows.push((e.index(), *y, h));
            if let Entry::Chips { members, .. } = e {
                for (m, _) in members {
                    placed.rows.push((*m, *y, h));
                }
            }
            *y += h;
        }
    };
    for s in &laid.sections {
        gap(&mut y);
        section(s, &mut y, &mut placed);
    }
    if adds {
        gap(&mut y);
        y += z.action_row;
    }
    if laid.folded > 0 {
        gap(&mut y);
        y += z.action_row;
        if open {
            for s in &laid.advanced {
                gap(&mut y);
                section(s, &mut y, &mut placed);
            }
        }
    }
    placed.height = y + z.pane_pad;
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use flower_core::{FigBackend, InlineBudget, ViewMode};

    const SAMPLE: &str = "\
title = \"flower\"
version = 1

# the server block
[server]
host = \"localhost\" # dev only
port = 8080
tags = [\"alpha\", \"beta\"]

[server.limits]
max_connections = 100
";

    fn model() -> Model<FigBackend> {
        let backend =
            FigBackend::open(SAMPLE.as_bytes(), fig_toml::format().unwrap()).expect("open");
        let mut m = Model::new(backend).expect("model");
        m.set_view(ViewMode::Pages);
        m.set_inline_budget(InlineBudget::default());
        m
    }

    fn open(m: &mut Model<FigBackend>, key: &str) {
        m.focus_on(&[Seg::Key(key.into())]);
        m.page_enter();
    }

    #[test]
    fn a_flat_document_is_one_pane_and_a_deep_one_two() {
        let flat = FigBackend::open(b"a = 1\nb = 2\n", fig_toml::format().unwrap()).unwrap();
        let mut flat = Model::new(flat).unwrap();
        flat.set_view(ViewMode::Pages);
        assert_eq!(panes(&flat, true).len(), 1);

        let m = model();
        let p = panes(&m, true);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].as_ref().unwrap().role, Role::Current);
        // And never two where there is no room for them.
        assert_eq!(panes(&m, false).len(), 1);
    }

    #[test]
    fn opening_a_page_slides_the_lineage_left() {
        let mut m = model();
        open(&mut m, "server");
        let p = panes(&m, true);
        let parent = p[0].as_ref().unwrap();
        assert_eq!(parent.role, Role::Parent);
        // The row the current page was opened through stays marked.
        let server = parent.page.items.iter().position(|i| i.label == "server");
        assert_eq!(parent.selected, server);
        assert_eq!(p[1].as_ref().unwrap().role, Role::Current);
    }

    #[test]
    fn a_short_list_is_one_row_of_chips_each_its_own_item() {
        let mut m = model();
        open(&mut m, "server");
        let laid = lay_out(m.page(), 500.0, &Sizes::BASE);
        let chips = laid
            .sections
            .iter()
            .flat_map(|s| &s.entries)
            .find(|e| matches!(e, Entry::Chips { .. }))
            .expect("a chips row");
        let Entry::Chips { members, .. } = chips else {
            unreachable!()
        };
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].1.preview, "alpha");
        assert!(chips.holds(members[1].0));
        // Too narrow for both beside the name: a section of rows instead.
        let narrow = lay_out(m.page(), 120.0, &Sizes::BASE);
        assert!(
            !narrow
                .sections
                .iter()
                .flat_map(|s| &s.entries)
                .any(|e| matches!(e, Entry::Chips { .. }))
        );
    }

    #[test]
    fn a_rank_zero_group_is_a_section_under_its_caption() {
        let src = "[server]\nhost = \"h\"\nport = 1\n";
        let b = FigBackend::open(src.as_bytes(), fig_toml::format().unwrap()).unwrap();
        let mut m = Model::new(b).unwrap();
        m.set_view(ViewMode::Pages);
        m.set_inline_budget(InlineBudget::default());
        let laid = lay_out(m.page(), 500.0, &Sizes::BASE);
        let titled = laid
            .sections
            .iter()
            .find(|s| s.header.is_some())
            .expect("a titled section");
        assert_eq!(titled.header.unwrap().1.label, "server");
        // Its members are rebased a rank left: the caption says whose they are.
        assert!(titled.entries.iter().all(|e| e.inset() == 0));
        assert_eq!(titled.entries.len(), 2);
    }

    #[test]
    fn keys_read_as_words_and_names_somebody_chose_do_not() {
        assert_eq!(prettify("max_connections"), "Max Connections");
        assert_eq!(prettify("noEmit"), "No Emit");
        assert_eq!(prettify("HTTPServer"), "HTTP Server");
        assert_eq!(prettify("node-version"), "Node Version");
        assert_eq!(prettify("has space"), "has space");
        assert_eq!(key_words("devDependencies"), ["dev", "Dependencies"]);

        let src = "[scripts]\ndev = \"vite\"\nbuild = \"vite build\"\n";
        let b = FigBackend::open(src.as_bytes(), fig_toml::format().unwrap()).unwrap();
        let mut m = Model::new(b).unwrap();
        m.set_view(ViewMode::Pages);
        m.set_inline_budget(InlineBudget::default());
        let page = m.page();
        let v = verbatim(page);
        let dev = page.items.iter().position(|i| i.label == "dev").unwrap();
        assert!(v.contains(&dev));
        assert_eq!(row_name(&page.items[dev], true), "dev");
    }

    #[test]
    fn a_container_shows_what_is_in_it_or_counts_it() {
        let mut m = model();
        open(&mut m, "server");
        let limits = m
            .page()
            .items
            .iter()
            .find(|i| i.label == "limits")
            .unwrap()
            .clone();
        let s = drill_summary(&limits);
        assert!(s.contains("max_connections") || s == "1 field", "{s}");
    }

    #[test]
    fn rows_are_placed_one_under_another_and_the_fold_hides_its_own() {
        let mut m = model();
        open(&mut m, "server");
        let laid = lay_out(m.page(), 500.0, &Sizes::BASE);
        let none = |_: &PageItem| 0;
        let placed = place(&laid, false, true, false, &none, &Sizes::BASE);
        let mut tops: Vec<f64> = placed.rows.iter().map(|r| r.1).collect();
        let sorted = {
            let mut t = tops.clone();
            t.sort_by(f64::total_cmp);
            t
        };
        assert_eq!(tops, sorted, "placed top to bottom");
        tops.dedup();
        // The first row starts under the pane's padding.
        assert_eq!(placed.rows[0].1, Sizes::BASE.pane_pad);
        // A row with a line under it is that much taller.
        let one = |_: &PageItem| 1;
        let taller = place(&laid, false, true, false, &one, &Sizes::BASE);
        assert!(taller.height > placed.height);
    }

    #[test]
    fn a_row_moves_only_among_its_siblings() {
        let mut m = model();
        open(&mut m, "server");
        let page = m.page();
        let first = 0;
        assert!(!can_move(page, first, -1));
        assert!(can_move(page, first, 1));
    }
}
