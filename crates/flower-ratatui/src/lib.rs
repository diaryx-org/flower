//! A ratatui **widget** over a [`flower_core::Model`]: a header line, the
//! document body, and a footer that doubles as the edit line — plus the keys
//! that drive them.
//!
//! The embedding app owns the terminal and the event loop; it calls [`draw`]
//! each frame and forwards key events to [`handle_key`] and mouse events to
//! [`handle_mouse`], which perform the navigation or edit the event implies and
//! return an [`Outcome`] naming what the *host* must do (quit, save) about the
//! ones the widget deliberately leaves alone. That is the same division as
//! `leaf-ratatui`'s, and for the same reason: a third-party TUI embedding
//! flower as a pane should get flower's key table by forwarding an event, not
//! by re-implementing the app's event loop — and its clicks by forwarding the
//! event and the `Rect` it drew into, not by restating the layout. `header` is
//! whatever the app wants to name the document (e.g. a file name) —
//! flower-core has no filesystem concept of its own.
//!
//! The body is the **page** projection ([`Model::page`](flower_core::Model::page))
//! — a settings-menu layout that is two panes when there is width and depth to
//! justify them, and one pane when there isn't. That fallback is a rendering
//! decision, not a mode: the model holds one page state and this decides how much
//! of it fits.
//!
//! How much a page inlines is a fact about the room, so the app is expected to
//! keep the model's budget sized to the terminal — [`page_room`] is the height
//! this crate's chrome leaves for it, and
//! [`Model::fit_to_room`](flower_core::Model::fit_to_room) is what reads it. A
//! document that fits the terminal is then drawn whole, with no navigation at
//! all, and the same document in a short one goes back to a page per level.
//!
//! It is the only projection drawn here. flower-core still offers the indented
//! tree, and an embedder that drives the model by row index still wants it, but a
//! terminal does not: depth costs an indent column the tree keeps paying on every
//! row below it, where a page spends it once on a breadcrumb. The app is expected
//! to hold the model in [`ViewMode::Pages`](flower_core::ViewMode::Pages), which
//! is what makes an edit resolve against the cursor this draws.

mod input;

pub use input::{Outcome, handle_key, handle_mouse};

use std::borrow::Cow;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

use flower_core::{Backend, EditSlot, ItemKind, Mode, Model, Page, PageItem, VKind};

/// Below this width a two-pane split leaves neither pane usable, so the page view
/// collapses to the single-pane (push/pop) layout — the same interaction, one
/// column, which is all a narrow terminal or a phone ever had room for.
const TWO_PANE_MIN_WIDTH: u16 = 64;

/// The rows [`draw`] spends on something other than a page's items: the header,
/// the footer, and the pane's own breadcrumb.
///
/// Subtracted from the terminal's height to get the room a page actually has,
/// which is what an inline budget wants to be measured against
/// ([`page_room`]).
const CHROME_ROWS: u16 = 3;

/// How many rows of items a page has, in a terminal `height` rows tall.
///
/// The app calls this to size the model's inline budget
/// ([`Model::fit_to_room`](flower_core::Model::fit_to_room)) — how much a page
/// inlines is a fact about the room, and this crate is what knows how much of
/// the room the chrome took. It is arithmetic on a layout constant rather than
/// on a measured `Rect` because the app needs the answer *before* the frame it
/// applies to, and the chrome is a fixed height either way.
pub fn page_room(height: u16) -> usize {
    height.saturating_sub(CHROME_ROWS) as usize
}

/// Kept clear on the right so a value never touches the pane's edge.
const RIGHT_GUTTER: usize = 1;

/// How the document root reads in a breadcrumb.
const ROOT_LABEL: &str = "‹document›";

/// Color a value preview by its kind.
fn value_style(kind: VKind) -> Style {
    let color = match kind {
        VKind::Null => Color::DarkGray,
        VKind::Bool => Color::Magenta,
        VKind::Int | VKind::Float => Color::Cyan,
        VKind::Str => Color::Green,
        VKind::Ext => Color::Yellow,
        VKind::Map | VKind::Seq => Color::DarkGray,
    };
    Style::default().fg(color)
}

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn key_style() -> Style {
    Style::default()
        .fg(Color::Blue)
        .add_modifier(Modifier::BOLD)
}

/// Render the whole editor into `f`. `header` names the document in the header
/// bar (typically the file name, plus whatever the app wants — e.g. format).
///
/// This is [`draw_in`] over the whole frame, which is what an app whose only
/// surface is this editor wants.
pub fn draw<B: Backend>(f: &mut Frame, model: &Model<B>, header: &str) {
    draw_in(f, f.area(), model, header);
}

/// Render the editor into `area` rather than into the whole frame — the same
/// picture, in a pane.
///
/// A host that draws this editor beside something else (a prose body, a file
/// list) owns the split and hands each side its `Rect`. The chrome is unchanged,
/// so [`page_room`] still describes the room: pass it the *pane's* height, not
/// the terminal's.
pub fn draw_in<B: Backend>(f: &mut Frame, area: Rect, model: &Model<B>, header: &str) {
    let [head, body, foot] = bands(area);
    draw_header(f, model, header, head);
    draw_pages(f, model, body);
    draw_footer(f, model, foot);
}

/// The three bands of the editor: a header line, the body, and the footer that
/// doubles as the edit line.
fn bands(area: Rect) -> [Rect; 3] {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area)
}

fn draw_header<B: Backend>(f: &mut Frame, model: &Model<B>, header: &str, area: Rect) {
    let dirty = if model.dirty { " ●" } else { "" };
    let title = format!(" flower — {}{}", header, dirty);
    let line = Line::from(Span::styled(
        title,
        Style::default()
            .fg(Color::Black)
            .bg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));
    f.render_widget(line, area);
}

// ── the page projection ──────────────────────────────────────────────────────

/// What a pane of the page layout stands for, relative to the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// The page the cursor is on.
    Current,
    /// The page one level out — the left pane, while the cursor's page is on
    /// the right.
    Parent,
    /// The page the cursor would open — the right pane, while the cursor's page
    /// leads the split and the right one is a preview.
    Peek,
}

/// One pane of the page layout: where it is, and the page it shows.
struct Pane<'m> {
    area: Rect,
    /// `None` is the placeholder pane — the right half while the cursor stands
    /// on a scalar, which has no page to preview.
    shows: Option<Shown<'m>>,
}

/// A page in a pane. The cursor is drawn only on the current page; the parent
/// keeps the row the current page was opened through marked instead, and a
/// preview marks nothing.
struct Shown<'m> {
    role: Role,
    page: Cow<'m, Page>,
    selected: Option<usize>,
}

/// The panes `area` is cut into, and what each shows — the one layout decision,
/// read by [`draw_pages`] to draw and by [`hit_at`] to resolve a click, so the
/// two cannot disagree about where a row went.
///
/// Two panes when the terminal is wide enough *and* there is something for the
/// second pane to hold; one otherwise. Both conditions matter. A flat document
/// has no categories to put in a sidebar, so splitting would spend half the
/// width drawing an empty box next to the only list there is.
fn panes<'m, B: Backend>(model: &'m Model<B>, area: Rect) -> Vec<Pane<'m>> {
    let current = |area: Rect| Pane {
        area,
        shows: Some(Shown {
            role: Role::Current,
            page: Cow::Borrowed(model.page()),
            selected: Some(model.page_selected()),
        }),
    };

    // The narrow layout: just the page you are on, with the breadcrumb standing
    // in for the sidebar you don't have room for.
    if area.width < TWO_PANE_MIN_WIDTH || model.pages_would_degenerate() {
        return vec![current(area)];
    }

    // Even halves. The two panes are consecutive levels of one lineage, not a
    // fixed index and a variable detail, so neither has a claim on more room than
    // the other — and the left is about to become the right.
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);

    // Exactly one pane owns the cursor: the left while the page you are choosing
    // on leads the split, the right once there is an outer page worth drawing
    // beside it. That is what makes `h` and `l` unambiguous without a
    // focus-switch key.
    if model.page_leads_the_split() {
        // Nothing worth drawing behind this page, so the right pane previews what
        // the cursor would open. Without it the split would start half empty.
        let peek = model.peek_page().map(|page| Shown {
            role: Role::Peek,
            page: Cow::Owned(page),
            selected: None,
        });
        return vec![
            current(left),
            Pane {
                area: right,
                shows: peek,
            },
        ];
    }

    // A window sliding along the lineage: the left pane is the page the right one
    // was opened from, at every depth. It keeps the row you came out of marked, so
    // a deep page never loses the trace of what contains it.
    let parent = model.parent_page();
    vec![
        Pane {
            area: left,
            shows: Some(Shown {
                role: Role::Parent,
                page: Cow::Borrowed(parent),
                selected: parent.position_of(model.focus()),
            }),
        },
        current(right),
    ]
}

fn draw_pages<B: Backend>(f: &mut Frame, model: &Model<B>, area: Rect) {
    for pane in panes(model, area) {
        match pane.shows {
            Some(shown) => draw_page_pane(f, &shown, pane.area),
            None => draw_empty_detail(f, pane.area),
        }
    }
}

/// A pane is a breadcrumb line over its items.
fn pane_rows(area: Rect) -> [Rect; 2] {
    Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area)
}

/// One page: a breadcrumb line, then its items. The breadcrumb of a pane that
/// does not hold the cursor is dimmed.
fn draw_page_pane(f: &mut Frame, shown: &Shown<'_>, area: Rect) {
    let [crumb, list] = pane_rows(area);

    let style = if shown.role == Role::Current {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        dim()
    };
    f.render_widget(
        Line::from(Span::styled(
            format!(" {}", shown.page.breadcrumb(ROOT_LABEL)),
            style,
        )),
        crumb,
    );

    if shown.page.is_empty() {
        f.render_widget(Line::from(Span::styled("   (empty)", dim())), list);
        return;
    }
    draw_list(f, &shown.page, shown.selected, list);
}

// ── laying a page out ────────────────────────────────────────────────────────

/// One drawable entry of a page: what the projection's flat, inset-tagged item
/// list becomes once it is read the way a settings screen reads.
///
/// This is the same seam the Swift host's `pageLayout` is. The projection ships
/// rows and group headers; a screen draws captions, rows, and chips. Three
/// rules, applied in document order:
///
/// - a **group** with members becomes a caption over them — its name, bold,
///   with a blank line above unless it opens the list — rather than a rule
///   across the pane, which gave every group the same weight whatever it held;
/// - a **scalar sequence** whose members fit on the row becomes one row of
///   chips, because a list of tags is one fact about the document, not `n`
///   rows of positions, and the file spends one line on it too;
/// - everything else — a scalar, a drill, an empty container — is one row.
///
/// Every member of a chips row is still its own item: the cursor walks the
/// chips as it walked the rows, and an edit resolves against the chip it is
/// on. Nothing here is a fact about the document, only about how it is drawn.
enum Entry<'p> {
    /// A group's name over the members inlined under it. `gap` is the blank
    /// row above it that separates it from what came before.
    Caption {
        index: usize,
        item: &'p PageItem,
        gap: bool,
    },
    /// One item as one row.
    Row { index: usize, item: &'p PageItem },
    /// A scalar sequence as one row: the header names it, the members are the
    /// chips. The one place several items share a line.
    Chips {
        index: usize,
        header: &'p PageItem,
        members: Vec<(usize, &'p PageItem)>,
    },
}

impl Entry<'_> {
    /// Whether the item at `selected` is drawn on this entry.
    fn holds(&self, selected: usize) -> bool {
        match self {
            Entry::Caption { index, .. } | Entry::Row { index, .. } => *index == selected,
            Entry::Chips { index, members, .. } => {
                *index == selected || members.iter().any(|(i, _)| *i == selected)
            }
        }
    }

    /// The item a click on this entry stands on when it lands on no chip in
    /// particular: the row's own, or a chips row's header.
    fn index(&self) -> usize {
        match self {
            Entry::Caption { index, .. } | Entry::Row { index, .. } => *index,
            Entry::Chips { index, .. } => *index,
        }
    }

    fn item(&self) -> &PageItem {
        match self {
            Entry::Caption { item, .. } | Entry::Row { item, .. } => item,
            Entry::Chips { header, .. } => header,
        }
    }

    /// The rows this entry takes: the gap above a caption, the comment
    /// written above the item, then the line itself.
    fn rows(&self) -> usize {
        let gap = matches!(self, Entry::Caption { gap: true, .. });
        usize::from(gap) + usize::from(self.item().leading_comment.is_some()) + 1
    }
}

/// The page's items as the entries a pane `width` columns wide draws.
fn layout(page: &Page, width: u16) -> Vec<Entry<'_>> {
    let items = &page.items;
    let mut out: Vec<Entry<'_>> = Vec::with_capacity(items.len());
    let mut i = 0;
    while i < items.len() {
        let item = &items[i];
        let ItemKind::GroupHeader { count } = item.kind else {
            out.push(Entry::Row { index: i, item });
            i += 1;
            continue;
        };
        let mut j = i + 1;
        while j < items.len() && items[j].inset > item.inset {
            j += 1;
        }
        let members: Vec<(usize, &PageItem)> = (i + 1..j).map(|k| (k, &items[k])).collect();
        // An empty container is a value, not a section: a caption over nothing
        // reads as a caption over whatever comes next.
        if count == 0 || members.is_empty() {
            out.push(Entry::Row { index: i, item });
            i += 1;
            continue;
        }
        if chips_fit(item, &members, width) {
            out.push(Entry::Chips {
                index: i,
                header: item,
                members,
            });
            i = j;
            continue;
        }
        out.push(Entry::Caption {
            index: i,
            item,
            gap: !out.is_empty(),
        });
        i += 1;
    }
    out
}

/// Whether a group is drawn as one row of chips: a scalar sequence, all of
/// whose members sit directly under it, short enough to share the row with its
/// name. A list that would not fit keeps a row per member, where each can be
/// read whole.
fn chips_fit(header: &PageItem, members: &[(usize, &PageItem)], width: u16) -> bool {
    if header.vkind != VKind::Seq
        || !members
            .iter()
            .all(|(_, m)| m.is_scalar() && m.inset == header.inset + 1)
    {
        return false;
    }
    let text: usize = members
        .iter()
        .map(|(_, m)| m.preview.chars().count())
        .sum::<usize>()
        + CHIP_SEP.chars().count() * (members.len() - 1);
    text <= room_for(indent_of(header), &name_spans(header), width)
}

/// What separates one chip from the next.
const CHIP_SEP: &str = " · ";

/// Where the values of a list start: one column, just past the widest name,
/// so the names read as one column and the values as another — the way the
/// file's `key = value` lines up, with the `=` taken out.
///
/// Capped at half the width so one long key cannot push every value off the
/// right edge; a name wider than that has its value follow it instead.
fn value_col(entries: &[Entry<'_>], width: u16) -> usize {
    let widest = entries
        .iter()
        .filter(|e| !matches!(e, Entry::Caption { .. }))
        .map(|e| indent_of(e.item()) + name_width(&name_spans(e.item())))
        .max()
        .unwrap_or(0);
    (widest + 2).min(usize::from(width) / 2)
}

fn indent_of(item: &PageItem) -> usize {
    2 + item.inset * 2
}

fn name_width(name: &[Span<'static>]) -> usize {
    name.iter().map(|s| s.content.chars().count()).sum()
}

// ── placing and drawing the entries ──────────────────────────────────────────

/// One entry of a page, placed in its list: which entry, and the rows it took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Placed {
    entry: usize,
    /// Rows from the top of the list.
    y: u16,
    height: u16,
}

/// Where each of `entries` lands in a list `height` rows tall.
///
/// The list starts at the top and scrolls only as far as it must to bring the
/// entry holding `selected` on screen — by whole entries, so no row is drawn
/// without the comment above it — which is the scroll a fresh `ListState`
/// gives ratatui's `List` on every frame, stated once here so that the frame
/// and a click resolved against it agree. An entry that does not fit is left
/// off, except the selected one, which is clipped rather than lost.
fn place(entries: &[Entry<'_>], selected: Option<usize>, height: u16) -> Vec<Placed> {
    let height = height as usize;
    let heights: Vec<usize> = entries.iter().map(Entry::rows).collect();
    let selected = selected.and_then(|s| entries.iter().position(|e| e.holds(s)));

    let mut first = 0;
    if let Some(s) = selected {
        let mut span: usize = heights[..=s].iter().sum();
        while span > height && first < s {
            span -= heights[first];
            first += 1;
        }
    }

    let mut placed = Vec::new();
    let mut y = 0;
    for (entry, &h) in heights.iter().enumerate().skip(first) {
        if y + h > height {
            if Some(entry) == selected && y < height {
                placed.push(Placed {
                    entry,
                    y: y as u16,
                    height: (height - y) as u16,
                });
            }
            break;
        }
        placed.push(Placed {
            entry,
            y: y as u16,
            height: h as u16,
        });
        y += h;
    }
    placed
}

/// The items of `page` as entries, `selected` highlighted.
fn draw_list(f: &mut Frame, page: &Page, selected: Option<usize>, area: Rect) {
    let highlight = Style::default()
        .bg(Color::Rgb(40, 40, 55))
        .add_modifier(Modifier::BOLD);
    let entries = layout(page, area.width);
    let col = value_col(&entries, area.width);
    for placed in place(&entries, selected, area.height) {
        let entry = &entries[placed.entry];
        let rect = Rect {
            x: area.x,
            y: area.y + placed.y,
            width: area.width,
            height: placed.height,
        };
        let drawn = render_entry(entry, col, area.width);
        f.render_widget(Text::from(drawn.lines), rect);
        let Some(selected) = selected.filter(|&s| entry.holds(s)) else {
            continue;
        };
        f.buffer_mut().set_style(rect, highlight);
        // A selected chip is marked on its own, the row being shared.
        if let Some(&(x0, x1, _)) = drawn.chips.iter().find(|(_, _, i)| *i == selected)
            && placed.height == entry.rows() as u16
        {
            let chip = Rect {
                x: area.x + x0 as u16,
                y: rect.y + rect.height - 1,
                width: (x1 - x0) as u16,
                height: 1,
            };
            f.buffer_mut()
                .set_style(chip, Style::default().add_modifier(Modifier::REVERSED));
        }
    }
}

/// What a point in the editor is standing on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// Row `i` of the page the cursor is on.
    Row(usize),
    /// Row `i` of the page one level out — the left pane, while the cursor's
    /// page is on the right.
    ParentRow(usize),
    /// Row `i` of the page the cursor would open — the right pane, while the
    /// cursor's page leads the split and the right one is a preview.
    PeekRow(usize),
}

/// Which row is drawn under `at`, in an editor drawn into `area` — the `Rect`
/// the host handed [`draw_in`], or the whole frame for [`draw`]. `None` for
/// the chrome (header, breadcrumbs, footer) and for the empty space under a
/// short list.
///
/// A row of chips is several items on one line: a point on a chip stands on
/// that member, and a point on the name, or between chips, on the sequence.
///
/// Resolved the way the frame was laid out, from the model alone: the widget
/// keeps no record of the last frame, so a host that draws and a host that
/// only asks get the same answer. [`handle_mouse`] is this with the click's
/// meaning applied; a host with a policy of its own — a click that also brings
/// the keyboard to the pane, say — reads the hit and applies its own.
pub fn hit_at<B: Backend>(model: &Model<B>, area: Rect, at: Position) -> Option<Hit> {
    let [_, body, _] = bands(area);
    let pane = panes(model, body)
        .into_iter()
        .find(|p| p.area.contains(at))?;
    let shown = pane.shows?;
    let [_, list] = pane_rows(pane.area);
    if !list.contains(at) {
        return None;
    }
    let y = at.y - list.y;
    let entries = layout(&shown.page, list.width);
    let placed = place(&entries, shown.selected, list.height)
        .into_iter()
        .find(|p| p.y <= y && y < p.y + p.height)?;
    let entry = &entries[placed.entry];
    let mut index = entry.index();
    if let Entry::Chips { .. } = entry
        && y == placed.y + entry.rows() as u16 - 1
    {
        let x = usize::from(at.x - list.x);
        let drawn = render_entry(entry, value_col(&entries, list.width), list.width);
        if let Some(&(_, _, i)) = drawn.chips.iter().find(|(x0, x1, _)| *x0 <= x && x < *x1) {
            index = i;
        }
    }
    Some(match shown.role {
        Role::Current => Hit::Row(index),
        Role::Parent => Hit::ParentRow(index),
        Role::Peek => Hit::PeekRow(index),
    })
}

fn draw_empty_detail(f: &mut Frame, area: Rect) {
    f.render_widget(Line::from(Span::styled("  select a section", dim())), area);
}

/// One entry, drawn: its lines, and where each chip landed on the last one.
struct Drawn {
    lines: Vec<Line<'static>>,
    /// `(from, to, item)` in columns from the pane's left edge, for the chips
    /// of a chips row — what a click on the row is resolved against.
    chips: Vec<(usize, usize, usize)>,
}

/// One entry as the rows it takes: a caption's gap, the comment written above
/// the item in the document when there is one, then the line itself.
///
/// The comment takes one row whatever its length — the first line of the block,
/// cut to the width — because a page is a list to scan, and a paragraph between
/// two of its rows would turn it into a page to read. The whole block is a
/// keystroke away (`C`), and the row's own line is never squeezed for it.
fn render_entry(entry: &Entry<'_>, col: usize, width: u16) -> Drawn {
    let item = entry.item();
    let indent = indent_of(item);
    let mut lines = Vec::with_capacity(3);
    if matches!(entry, Entry::Caption { gap: true, .. }) {
        lines.push(Line::default());
    }
    if let Some(comment) = &item.leading_comment {
        let first = comment.lines().next().unwrap_or_default();
        let text = fit(
            &format!("# {first}"),
            (width as usize).saturating_sub(indent + RIGHT_GUTTER),
        );
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(indent)),
            Span::styled(text, dim()),
        ]));
    }
    let mut chips = Vec::new();
    lines.push(match entry {
        Entry::Caption { item, .. } => caption_line(item, width),
        Entry::Row { item, .. } => row_line(item, col, width),
        Entry::Chips {
            header, members, ..
        } => {
            let (line, at) = chips_line(header, members, col, width);
            chips = at;
            line
        }
    });
    Drawn { lines, chips }
}

/// A group's name over its members: bold, on a line of its own, with the
/// file's aside on it dimmed after — enough to bind the members below it
/// together without a rule across the pane or a second indentation scheme.
///
/// No drill chevron, though a caption does open a page: the members it would
/// lead to are already on screen, which is the whole point of inlining them.
fn caption_line(item: &PageItem, width: u16) -> Line<'static> {
    let indent = indent_of(item);
    let name = name_spans(item);
    let mut spans = vec![Span::raw(" ".repeat(indent))];
    spans.extend(name.clone());
    if let Some(comment) = &item.trailing_comment {
        let room = room_for(indent, &name, width).saturating_sub(2);
        let text = fit(&format!("# {comment}"), room);
        if text.chars().count() >= 4 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(text, dim()));
        }
    }
    Line::from(spans)
}

/// One item as one row, the way a settings row reads: the name on the left,
/// its value or affordance in the value column.
fn row_line(item: &PageItem, col: usize, width: u16) -> Line<'static> {
    let name = name_spans(item);
    match &item.kind {
        ItemKind::Scalar => tail_line(
            item,
            name,
            &item.preview,
            value_style(item.vkind),
            col,
            width,
        ),
        // An empty container, drawn as the value it is.
        ItemKind::GroupHeader { .. } => {
            let empty = if item.vkind == VKind::Seq { "[]" } else { "{}" };
            tail_line(item, name, empty, dim(), col, width)
        }
        ItemKind::Drill { count } => {
            // Show what is in it when what is in it fits: `1 field ›` is strictly
            // less than the document says when the field is right there. The count
            // is the fallback for a container too big to put on the row, which is
            // the only case where counting beats showing.
            let trailing = match &item.summary {
                Some(flow)
                    if flow.chars().count() + 2 <= room_for(indent_of(item), &name, width) =>
                {
                    format!("{flow} ›")
                }
                _ => {
                    let noun = match (item.vkind == VKind::Seq, *count) {
                        (true, 1) => "item",
                        (true, _) => "items",
                        (false, 1) => "field",
                        (false, _) => "fields",
                    };
                    format!("{count} {noun} ›")
                }
            };
            tail_line(item, name, &trailing, dim(), col, width)
        }
    }
}

/// A scalar sequence on one row: its name, then each member as a chip, in the
/// value column. What does not fit is cut with an ellipsis — the sequence was
/// only folded because it fit against its own name, so this is the value
/// column having moved it, and the whole list is still `l` away.
fn chips_line(
    header: &PageItem,
    members: &[(usize, &PageItem)],
    col: usize,
    width: u16,
) -> (Line<'static>, Vec<(usize, usize, usize)>) {
    let indent = indent_of(header);
    let name = name_spans(header);
    let start = tail_start(indent, &name, col);
    let end = usize::from(width).saturating_sub(RIGHT_GUTTER);

    let mut spans = vec![Span::raw(" ".repeat(indent))];
    let name_w = name_width(&name);
    spans.extend(name);
    spans.push(Span::raw(" ".repeat(start - indent - name_w)));
    let mut at = Vec::with_capacity(members.len());
    let mut x = start;
    for (n, (index, member)) in members.iter().enumerate() {
        if n > 0 {
            let sep = fit(CHIP_SEP, end.saturating_sub(x));
            x += sep.chars().count();
            spans.push(Span::styled(sep, dim()));
        }
        let text = fit(&member.preview, end.saturating_sub(x));
        if text.is_empty() {
            break;
        }
        let w = text.chars().count();
        at.push((x, x + w, *index));
        spans.push(Span::styled(text, value_style(member.vkind)));
        x += w;
    }
    (Line::from(spans), at)
}

/// `indent + name   trailing  # comment`, the trailing text in the value
/// column and truncated before the name is ever squeezed — a name you can't
/// read costs more than a value you can't finish.
///
/// The comment goes first when room runs short: it is the file's aside on the
/// value, and an aside is what you drop before the thing it is about. It is
/// shown dimmed after the value, the way it sits in the file, and not at all
/// when fewer than a few characters of it would fit.
fn tail_line(
    item: &PageItem,
    name: Vec<Span<'static>>,
    trailing: &str,
    trailing_style: Style,
    col: usize,
    width: u16,
) -> Line<'static> {
    let indent = indent_of(item);
    let start = tail_start(indent, &name, col);
    let room = usize::from(width).saturating_sub(start + RIGHT_GUTTER);

    let trailing = fit(trailing, room);
    let comment = item
        .trailing_comment
        .as_deref()
        .map(|c| format!("# {c}"))
        .map(|c| fit(&c, room.saturating_sub(trailing.chars().count() + 2)))
        .filter(|c| c.chars().count() >= 4);

    let mut spans = vec![Span::raw(" ".repeat(indent))];
    let name_w = name_width(&name);
    spans.extend(name);
    spans.push(Span::raw(" ".repeat(start - indent - name_w)));
    spans.push(Span::styled(trailing, trailing_style));
    if let Some(comment) = comment {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(comment, dim()));
    }
    Line::from(spans)
}

/// The column a row's trailing text starts in: the page's value column, or
/// two past the name when the name reaches beyond it.
fn tail_start(indent: usize, name: &[Span<'static>], col: usize) -> usize {
    (indent + name_width(name) + 2).max(col)
}

/// What names an item on the left of its row.
///
/// A titled sequence item keeps its index *and* gains the title: the index is
/// what the path addresses and what a reorder moves, so dropping it would leave
/// nothing to reconcile the row with the document — but it is dimmed, because on
/// a list of twenty steps the title is what you are reading and the index is what
/// you check afterwards.
fn name_spans(item: &PageItem) -> Vec<Span<'static>> {
    match &item.title {
        Some(title) => vec![
            Span::styled(format!("{} ", item.label), dim()),
            Span::styled(title.clone(), key_style()),
        ],
        None => vec![Span::styled(item.label.clone(), key_style())],
    }
}

/// `text` cut to `room` columns, with an ellipsis where it was cut. Empty when
/// there is no room for even the ellipsis and one character.
fn fit(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        text.to_string()
    } else if room >= 2 {
        text.chars().take(room - 1).chain(['…']).collect()
    } else {
        String::new()
    }
}

/// How many columns are left for a row's trailing text once its name has taken
/// what it needs — the name is never squeezed, because a name you can't read
/// costs more than a value you can't finish.
fn room_for(indent: usize, name: &[Span<'static>], width: u16) -> usize {
    (width as usize).saturating_sub(indent + name_width(name) + RIGHT_GUTTER + 2)
}

// ── footer ───────────────────────────────────────────────────────────────────

fn draw_footer<B: Backend>(f: &mut Frame, model: &Model<B>, area: Rect) {
    // Kept short enough to survive an 80-column terminal alongside the status.
    let hints = "  j/k · l/h · e edit · c/C comment · x del · u/U undo · s save · q quit";
    let line = match &model.mode {
        Mode::Editing { buffer, slot, .. } => {
            let badge = match slot {
                EditSlot::Value => " edit ",
                EditSlot::TrailingComment => " comment ",
                EditSlot::LeadingComment => " comment above ",
            };
            // A leading block may span lines, and the footer is one: the breaks
            // are shown as a mark rather than lost, so the block is at least
            // read and trimmed faithfully here. Adding a line is not on offer.
            let shown = buffer.replace('\n', "⏎");
            Line::from(vec![
                Span::styled(badge, Style::default().bg(Color::Yellow).fg(Color::Black)),
                Span::raw(" "),
                Span::raw(shown),
                Span::styled("▏", Style::default().fg(Color::Yellow)),
                Span::styled("   (Enter to commit · Esc to cancel)", dim()),
            ])
        }
        // The status carries refusals, and is empty until there has been one —
        // so the badge is drawn only when it has something in it. A green block
        // holding two spaces is a widget reporting that nothing is wrong, which
        // is the state the footer is in almost all the time.
        Mode::Normal => {
            let mut spans = Vec::new();
            if !model.status.is_empty() {
                spans.push(Span::styled(
                    format!(" {} ", model.status),
                    Style::default().fg(Color::Black).bg(Color::Green),
                ));
            }
            spans.push(Span::styled(hints, dim()));
            Line::from(spans)
        }
    };
    f.render_widget(line, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    // The app holds the model in the page projection (see the module docs), so
    // the tests set it up the same way — rendering reads the page state either
    // way, but an edit routed from a test model should route where the app's
    // would.
    use flower_core::{FigBackend, Seg, ViewMode};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

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

    fn model() -> Model<FigBackend> {
        let backend = FigBackend::open(SAMPLE.as_bytes(), fig::Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        model
    }

    /// Render at `w × h` and return the buffer as text, trailing blanks trimmed.
    fn render(model: &Model<FigBackend>, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("terminal");
        terminal
            .draw(|f| draw(f, model, "sample.toml"))
            .expect("draw");
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn wide_renders_two_panes_with_a_preview_of_the_selection() {
        let mut model = model();
        for _ in 0..3 {
            model.page_move_down();
        }
        let out = render(&model, 76, 12);
        // Sidebar on the left, the page the cursor would open on the right.
        assert!(out.contains("‹document›"), "{out}");
        assert!(out.contains("server"), "{out}");
        assert!(out.contains("max_connections"), "{out}");
        // Values sit in a column of their own, not written as `key = value`.
        assert!(!out.contains("host = "), "{out}");
    }

    /// There is one projection here, and no key to ask for another.
    #[test]
    fn the_page_is_drawn_whatever_view_the_model_is_in() {
        let mut model = model();
        // Deliberately the *other* view: flower-core still has a tree, and this
        // is what says the widget no longer has a renderer to fall into.
        model.set_view(ViewMode::Tree);
        let out = render(&model, 76, 14);
        // A page — a breadcrumb, and values in their own column rather than `key = value`
        // rows under expand twisties.
        assert!(out.contains("‹document›"), "{out}");
        assert!(!out.contains("host = "), "{out}");
        assert!(!out.contains("▾ "), "{out}");
        assert!(!out.contains("v pages") && !out.contains("v tree"), "{out}");
    }

    #[test]
    fn comments_are_drawn_above_and_after_the_rows_they_belong_to() {
        let src = "\
# what it is called
title = \"flower\"
port = 8080 # dev only
";
        let backend = FigBackend::open(src.as_bytes(), fig::Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        let out = render(&model, 60, 8);
        let lines: Vec<&str> = out.lines().collect();
        let above = lines
            .iter()
            .position(|l| l.contains("# what it is called"))
            .expect(&out);
        let title = lines.iter().position(|l| l.contains("title")).expect(&out);
        assert_eq!(above + 1, title, "the block sits on the row above:\n{out}");
        assert!(out.contains("8080  # dev only"), "{out}");
    }

    #[test]
    fn a_trailing_comment_gives_way_before_the_value_does() {
        let src = "port = 8080 # a comment far longer than the row has room for\n";
        let backend = FigBackend::open(src.as_bytes(), fig::Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        let out = render(&model, 30, 6);
        assert!(out.contains("8080"), "{out}");
        assert!(
            out.contains("…"),
            "the comment is cut, not the value:\n{out}"
        );
    }

    #[test]
    fn a_group_is_inlined_under_its_name_and_a_tag_list_is_one_row() {
        let mut model = model();
        model.focus_on(&[Seg::Key("server".into())]);
        model.page_enter();
        let out = render(&model, 76, 12);
        // The group's name is a caption over its members, not a rule across
        // the pane, and a blank row sets it off from the rows above.
        assert!(!out.contains("──"), "{out}");
        let lines: Vec<&str> = out.lines().collect();
        let limits = lines.iter().position(|l| l.contains("limits")).expect(&out);
        // The right pane starts at the midpoint; the left one is still drawn.
        let right = |l: &str| l.chars().skip(38).collect::<String>();
        assert_eq!(right(lines[limits - 1]).trim_end(), "", "{out}");
        assert!(lines[limits + 1].contains("max_connections"), "{out}");
        // A scalar sequence is one row, its members as chips: the file spends
        // one line on `tags`, and so does the page.
        assert!(out.contains("tags"), "{out}");
        assert!(out.contains("alpha · beta"), "{out}");
        assert!(!out.contains("[0]"), "{out}");
    }

    /// The `languages.figl` shape: a list of entries, each with an empty list
    /// and a short one.
    fn languages_model() -> Model<FigBackend> {
        let src = br#"{"language": [
            {"name": "js-gitconfig", "extensions": [],
             "command": ["fig-quickjs", "~/diaryx/fig-quickjs/languages/gitconfig.mjs"]},
            {"name": "js-sshconfig", "extensions": [],
             "command": ["fig-quickjs", "~/diaryx/fig-quickjs/languages/sshconfig.mjs"]}
        ]}"#;
        let backend = FigBackend::open(src, fig::Format::Json).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        model.fit_to_room(page_room(30));
        model.enter_document();
        model
    }

    #[test]
    fn a_list_of_small_entries_reads_like_the_file() {
        let out = render(&languages_model(), 100, 30);
        let lines: Vec<&str> = out.lines().collect();
        // Each entry: its title as a caption, then one row per field — three,
        // as the file has — with the empty list drawn as the value it is and
        // the command as chips on one row.
        let entry = lines
            .iter()
            .position(|l| l.contains("js-gitconfig"))
            .expect(&out);
        assert!(lines[entry].contains("[0] js-gitconfig"), "{out}");
        assert!(
            lines[entry + 1].contains("name") && lines[entry + 1].contains("js-gitconfig"),
            "{out}"
        );
        assert!(
            lines[entry + 2].contains("extensions") && lines[entry + 2].contains("[]"),
            "{out}"
        );
        assert!(lines[entry + 3].contains("command"), "{out}");
        assert!(
            lines[entry + 3].contains("fig-quickjs · ~/diaryx/fig-quickjs/languages/gitconfig.mjs"),
            "{out}"
        );
        assert_eq!(
            lines[entry + 4].trim_end(),
            "",
            "a blank row before the next entry:\n{out}"
        );
        assert!(lines[entry + 5].contains("[1] js-sshconfig"), "{out}");
        assert!(!out.contains("──"), "{out}");
        // Values line up in one column just past the widest name, not at the
        // far edge of a wide terminal.
        let value_at = |l: &str| l.find("js-gitconfig").expect(l);
        assert_eq!(
            value_at(lines[entry + 1]),
            lines[entry + 2].find("[]").expect(&out)
        );
        assert_eq!(
            value_at(lines[entry + 1]),
            lines[entry + 3].find("fig-quickjs").expect(&out)
        );
        assert!(value_at(lines[entry + 1]) < 20, "{out}");
    }

    #[test]
    fn a_chip_is_the_item_it_stands_for() {
        let mut m = languages_model();
        let area = Rect::new(0, 0, 100, 30);
        let out = render(&m, 100, 30);
        let lines: Vec<&str> = out.lines().collect();
        let row = lines
            .iter()
            .position(|l| l.contains("fig-quickjs ·"))
            .expect(&out);
        let line = lines[row];
        let name = line.find("command").expect(line) as u16;
        let first = line.find("fig-quickjs").expect(line) as u16;
        let second = line.find("~/diaryx").expect(line) as u16;
        let y = row as u16;
        // The page is `language`'s own (the root's one row was stepped past):
        // [0] (0), name (1), extensions (2), command (3), its members (4, 5).
        assert_eq!(hit_at(&m, area, at(name, y)), Some(Hit::Row(3)));
        assert_eq!(hit_at(&m, area, at(first, y)), Some(Hit::Row(4)));
        assert_eq!(hit_at(&m, area, at(second + 3, y)), Some(Hit::Row(5)));
        // Between two chips is the sequence.
        assert_eq!(hit_at(&m, area, at(second - 2, y)), Some(Hit::Row(3)));
        // And the cursor walks the chips as items: standing on a member draws
        // the row as selected, and an edit is of that member.
        for _ in 0..4 {
            m.page_move_down();
        }
        assert_eq!(m.page_selected(), 4);
        assert_eq!(
            m.page_item().map(|i| i.preview.as_str()),
            Some("fig-quickjs")
        );
    }

    #[test]
    fn a_tag_list_too_long_for_its_row_keeps_a_row_per_member() {
        let tags: Vec<String> = (0..12).map(|i| format!("\"tag-number-{i}\"")).collect();
        let src = format!("tags = [{}]\n", tags.join(", "));
        let backend = FigBackend::open(src.as_bytes(), fig::Format::Toml).expect("open");
        let mut m = Model::new(backend).expect("model");
        m.set_view(ViewMode::Pages);
        m.fit_to_room(page_room(30));
        let out = render(&m, 60, 30);
        // No chips: the list would not fit the row, so each member is a row
        // that can be read whole.
        assert!(!out.contains("tag-number-0 ·"), "{out}");
        assert!(out.contains("[0]") && out.contains("[11]"), "{out}");
        assert!(out.contains("tag-number-11"), "{out}");
    }

    #[test]
    fn narrow_collapses_to_one_pane_and_keeps_the_breadcrumb() {
        let mut model = model();
        model.focus_on(&[Seg::Key("server".into())]);
        model.page_enter();
        let out = render(&model, 40, 12);
        assert!(out.contains("server"), "{out}");
        assert!(out.contains("localhost"), "{out}");
        // One pane: nothing is drawn past the single column's width.
        assert!(out.lines().all(|l| l.chars().count() <= 40), "{out}");
    }

    #[test]
    fn a_flat_document_gets_the_whole_width() {
        let backend = FigBackend::open(b"alpha = 1\nbeta = 2\n", fig::Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        let out = render(&model, 76, 8);
        assert!(model.pages_would_degenerate());
        assert!(out.contains("alpha"), "{out}");
        // No sidebar/detail split, so no second breadcrumb.
        assert_eq!(out.matches("‹document›").count(), 1, "{out}");
    }

    /// The `repos.figl` shape: one key holding a list of small entries, too
    /// many of them to inline.
    fn list_model() -> Model<FigBackend> {
        let items: Vec<String> = (0..22)
            .map(|i| format!(r#"{{"name": "r{i}", "lang": "rust"}}"#))
            .collect();
        let src = format!(r#"{{"repo": [{}]}}"#, items.join(", "));
        let backend = FigBackend::open(src.as_bytes(), fig::Format::Json).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        model.fit_to_room(page_room(24));
        model.enter_document();
        model
    }

    #[test]
    fn a_list_of_entries_is_a_list_of_rows_not_a_wall_of_groups() {
        let out = render(&list_model(), 100, 24);
        // One row per entry, each named by its `name` field and showing the
        // rest of itself — not a titled rule and two indented rows apiece.
        assert!(out.contains("[0] r0"), "{out}");
        assert!(out.contains("[20] r20"), "{out}");
        assert!(!out.contains("──"), "{out}");
        // The title is not repeated inside the summary that follows it.
        assert!(out.contains("{lang: rust} ›"), "{out}");
        assert!(!out.contains("name: r0"), "{out}");
    }

    #[test]
    fn a_document_that_is_one_list_spends_neither_pane_on_naming_itself() {
        let out = render(&list_model(), 100, 24);
        // Left: the entries, with the cursor. Right: the first one's fields.
        // Neither pane is the root page, whose only row named the file.
        let crumbs = out.lines().nth(1).expect("breadcrumb row");
        assert!(crumbs.contains("repo"), "{crumbs:?}");
        assert!(!crumbs.contains(ROOT_LABEL), "{crumbs:?}");
        assert!(out.contains("r0"), "{out}");
        // The preview pane holds the selected entry's own fields.
        assert!(out.contains("lang"), "{out}");
    }

    #[test]
    fn a_document_that_fits_the_room_is_drawn_whole_on_one_pane() {
        let mut model = model();
        model.fit_to_room(page_room(20));
        let out = render(&model, 100, 20);
        // Every value on screen, no drill affordance, and no second pane to
        // navigate with — the whole file, at full width.
        assert!(out.contains("localhost") && out.contains("30.5"), "{out}");
        // No drill affordance: the chevron always trails a space, where the
        // one in `‹document›` trails a letter.
        assert!(!out.contains(" ›"), "{out}");
        assert_eq!(out.matches(ROOT_LABEL).count(), 1, "{out}");
    }

    #[test]
    fn the_same_document_in_a_short_terminal_goes_back_to_drilling() {
        let mut model = model();
        model.fit_to_room(page_room(9));
        let out = render(&model, 100, 9);
        assert!(!out.contains("localhost"), "{out}");
        assert!(out.contains("server"), "{out}");
    }

    #[test]
    fn a_long_value_is_truncated_before_the_key_is() {
        let backend = FigBackend::open(
            format!("key = \"{}\"\n", "x".repeat(200)).as_bytes(),
            fig::Format::Toml,
        )
        .expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        let out = render(&model, 40, 6);
        assert!(out.contains("key"), "{out}");
        assert!(out.contains('…'), "{out}");
        assert!(out.lines().all(|l| l.chars().count() <= 40), "{out}");
    }

    #[test]
    fn a_sequence_of_mappings_is_listed_by_title_not_by_index() {
        let src = br#"{"steps": [
            {"uses": "actions/checkout@v7"},
            {"uses": "mlugg/setup-zig@v2", "with": {"version": "0.16.0"}},
            {"run": "cargo xtask ci"}
        ]}"#;
        let backend = FigBackend::open(src, fig::Format::Json).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        model.focus_on(&[Seg::Key("steps".into())]);
        model.page_enter();

        let out = render(&model, 76, 10);
        assert!(out.contains("actions/checkout@v7"), "{out}");
        assert!(out.contains("cargo xtask ci"), "{out}");
        // The index stays alongside the title — it is what a reorder moves.
        assert!(out.contains("[0]"), "{out}");
        // Uniform: every item is a drill row, none expanded inline.
        assert!(!out.contains("──"), "{out}");
        // And the count reads as English.
        assert!(
            out.contains("1 field ›") && !out.contains("1 fields"),
            "{out}"
        );
    }

    #[test]
    fn the_left_pane_is_the_page_the_right_one_was_opened_from() {
        let mut m = model();
        m.focus_on(&[Seg::Key("server".into())]);
        m.page_enter();
        let out = render(&m, 100, 10);
        // Left: the root page it came out of. Right: server's own page. Neither
        // pane repeats the other.
        assert!(out.contains("‹document›"), "{out}");
        assert!(out.contains(" server"), "{out}");
        assert_eq!(out.matches("localhost").count(), 1, "{out}");
        // Even halves: the right pane's breadcrumb starts at the midpoint.
        // Even halves: the right pane's breadcrumb sits one column into the
        // second half. Counted in characters, not bytes — `‹document›` is wider
        // in bytes than it is on screen.
        let crumb = out.lines().nth(1).expect("breadcrumb row");
        let col = crumb
            .char_indices()
            .position(|(i, _)| crumb[i..].starts_with("server"))
            .expect("the right pane's breadcrumb");
        assert_eq!(col, 51, "{crumb:?}");
    }

    // ── hit-testing ──────────────────────────────────────────────────────

    fn at(x: u16, y: u16) -> Position {
        Position::new(x, y)
    }

    #[test]
    fn a_hit_names_the_row_under_the_point_and_nothing_under_the_chrome() {
        let m = model();
        let area = Rect::new(0, 0, 40, 12);
        // Row 0 is the header, row 1 the breadcrumb; the items start at row 2.
        assert_eq!(hit_at(&m, area, at(3, 0)), None);
        assert_eq!(hit_at(&m, area, at(3, 1)), None);
        assert_eq!(hit_at(&m, area, at(3, 2)), Some(Hit::Row(0)));
        assert_eq!(hit_at(&m, area, at(30, 5)), Some(Hit::Row(3)));
        // The footer, and the empty space under a short list.
        assert_eq!(hit_at(&m, area, at(3, 11)), None);
        assert_eq!(hit_at(&m, area, at(3, 10)), None);
        // Outside the editor altogether.
        assert_eq!(hit_at(&m, area, at(45, 3)), None);
    }

    #[test]
    fn a_hit_is_resolved_in_the_rect_the_editor_was_drawn_into() {
        // Embedded at an offset: the same rows, found from the pane's origin
        // rather than the frame's.
        let m = model();
        let area = Rect::new(10, 5, 40, 12);
        assert_eq!(hit_at(&m, area, at(3, 7)), None);
        assert_eq!(hit_at(&m, area, at(12, 7)), Some(Hit::Row(0)));
        assert_eq!(hit_at(&m, area, at(12, 8)), Some(Hit::Row(1)));
    }

    #[test]
    fn a_comment_above_a_row_is_part_of_the_row_it_is_above() {
        let src = "\
# what it is called
title = \"flower\"
port = 8080
";
        let backend = FigBackend::open(src.as_bytes(), fig::Format::Toml).expect("open");
        let mut m = Model::new(backend).expect("model");
        m.set_view(ViewMode::Pages);
        let area = Rect::new(0, 0, 40, 8);
        // Rows 2 and 3 are both `title`: the comment, then the row. `port` is
        // on row 4, not 3 — every item is *not* one row.
        assert_eq!(hit_at(&m, area, at(3, 2)), Some(Hit::Row(0)));
        assert_eq!(hit_at(&m, area, at(3, 3)), Some(Hit::Row(0)));
        assert_eq!(hit_at(&m, area, at(3, 4)), Some(Hit::Row(1)));
        assert_eq!(hit_at(&m, area, at(3, 5)), None);
    }

    #[test]
    fn a_hit_on_a_scrolled_list_accounts_for_the_scroll() {
        let mut m = list_model();
        for _ in 0..15 {
            m.page_move_down();
        }
        // 24 rows: header, breadcrumb, 21 of list, footer. The selection is
        // item 15, which fits without scrolling — so the top row is item 0.
        let area = Rect::new(0, 0, 100, 24);
        assert_eq!(hit_at(&m, area, at(3, 2)), Some(Hit::Row(0)));
        assert_eq!(hit_at(&m, area, at(3, 17)), Some(Hit::Row(15)));
        // Standing on the last of 22 items scrolls the list by one, and what
        // is drawn on the top row is what a click there stands on.
        for _ in 0..6 {
            m.page_move_down();
        }
        assert_eq!(m.page_selected(), 21);
        let out = render(&m, 100, 24);
        assert!(!out.contains("[0] r0"), "{out}");
        assert_eq!(hit_at(&m, area, at(3, 2)), Some(Hit::Row(1)));
        assert_eq!(hit_at(&m, area, at(3, 22)), Some(Hit::Row(21)));
    }

    #[test]
    fn the_other_pane_names_its_rows_by_what_it_shows() {
        // The page leads the split: the right pane previews what the cursor
        // would open.
        let mut m = list_model();
        let area = Rect::new(0, 0, 100, 24);
        assert!(m.page_leads_the_split());
        assert_eq!(hit_at(&m, area, at(3, 2)), Some(Hit::Row(0)));
        assert_eq!(hit_at(&m, area, at(60, 2)), Some(Hit::PeekRow(0)));
        assert_eq!(hit_at(&m, area, at(60, 3)), Some(Hit::PeekRow(1)));
        assert_eq!(hit_at(&m, area, at(60, 4)), None);

        // The page was opened from the one on the left: that pane is the
        // parent, and the cursor's page is on the right.
        m = model();
        m.focus_on(&[Seg::Key("server".into())]);
        m.page_enter();
        assert!(!m.page_leads_the_split());
        assert_eq!(hit_at(&m, area, at(3, 2)), Some(Hit::ParentRow(0)));
        assert_eq!(hit_at(&m, area, at(60, 2)), Some(Hit::Row(0)));
    }

    #[test]
    fn a_scalar_under_the_cursor_leaves_the_preview_pane_empty() {
        let mut m = model();
        // A short room, so the root page has drills and keeps its split; the
        // cursor starts on `title`, a scalar with no page to preview.
        m.fit_to_room(page_room(9));
        let area = Rect::new(0, 0, 100, 9);
        assert!(m.page_item().is_some_and(PageItem::is_scalar));
        assert!(render(&m, 100, 9).contains("select a section"));
        assert_eq!(hit_at(&m, area, at(60, 2)), None);
    }

    #[test]
    fn a_small_container_shows_its_contents_on_the_row() {
        let backend = FigBackend::open(
            br#"{"on": {"push": {"branches": ["main"]}, "jobs": {"a": {"b": {"c": 1}}}}}"#,
            fig::Format::Json,
        )
        .expect("open");
        let mut m = Model::new(backend).expect("model");
        m.set_view(ViewMode::Pages);
        m.focus_on(&[Seg::Key("on".into())]);
        m.page_enter();
        let out = render(&m, 100, 10);
        assert!(out.contains("{branches: [main]} ›"), "{out}");
        assert!(!out.contains("1 field ›"), "{out}");
    }
}
