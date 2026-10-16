//! Laying a page out: which panes there are, what each entry of a page is, how
//! many lines it takes, where it lands in its pane and what each of its lines
//! says. No Dioxus type is named here, so all of it is tested without a window.
//!
//! This is the same seam `flower-ratatui`'s layout and the Swift host's
//! `pageLayout` are. The projection ships a flat list of items with insets and
//! group headers; a screen draws captions, rows and chips. The rules are the
//! terminal's, in the same words, because the type here is monospace and a
//! line is a row of cells: a group with members is a caption over them, a
//! scalar list that fits on its own row is one row of chips, everything else
//! is one row, and every name is drawn whole before any value is.

use std::borrow::Cow;

use flower_core::annotate::Severity;
use flower_core::{Backend, ItemKind, Model, Page, PageItem, VKind};

/// Kept clear on the right so a value never touches the pane's edge.
const RIGHT_GUTTER: usize = 1;

/// What separates one chip from the next.
const CHIP_SEP: &str = " · ";

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

/// One drawable entry of a page.
#[derive(Debug)]
pub enum Entry<'p> {
    /// A group's name over the members inlined under it. `gap` is the blank
    /// line above it that separates it from what came before.
    Caption {
        index: usize,
        item: &'p PageItem,
        gap: bool,
    },
    /// One item as one row.
    Row { index: usize, item: &'p PageItem },
    /// A scalar sequence as one row: the header names it, the members are the
    /// chips. Every member is still its own item, which the cursor walks.
    Chips {
        index: usize,
        header: &'p PageItem,
        members: Vec<(usize, &'p PageItem)>,
    },
}

impl Entry<'_> {
    /// Whether the item at `selected` is drawn on this entry.
    pub fn holds(&self, selected: usize) -> bool {
        match self {
            Entry::Caption { index, .. } | Entry::Row { index, .. } => *index == selected,
            Entry::Chips { index, members, .. } => {
                *index == selected || members.iter().any(|(i, _)| *i == selected)
            }
        }
    }

    /// The item a click on this entry stands on when it lands on no chip in
    /// particular: the row's own, or a chips row's header.
    pub fn index(&self) -> usize {
        match self {
            Entry::Caption { index, .. }
            | Entry::Row { index, .. }
            | Entry::Chips { index, .. } => *index,
        }
    }

    pub fn item(&self) -> &PageItem {
        match self {
            Entry::Caption { item, .. } | Entry::Row { item, .. } => item,
            Entry::Chips { header, .. } => header,
        }
    }

    /// The lines this entry takes: the gap above a caption, the comment
    /// written above the item, then the line itself.
    pub fn lines(&self) -> usize {
        let gap = matches!(self, Entry::Caption { gap: true, .. });
        usize::from(gap) + usize::from(self.item().leading_comment.is_some()) + 1
    }
}

/// The page's items as the entries a pane `cells` wide draws.
pub fn entries(page: &Page, cells: usize) -> Vec<Entry<'_>> {
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
        // An empty container is a value, not a section.
        if count == 0 || members.is_empty() {
            out.push(Entry::Row { index: i, item });
            i += 1;
            continue;
        }
        if chips_fit(item, &members, cells) {
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
/// whose members sit directly under it, short enough to share the row with
/// its name.
fn chips_fit(header: &PageItem, members: &[(usize, &PageItem)], cells: usize) -> bool {
    if header.vkind != VKind::Seq
        || !members
            .iter()
            .all(|(_, m)| m.is_scalar() && m.inset == header.inset + 1)
    {
        return false;
    }
    let text: usize = members
        .iter()
        .map(|(_, m)| width(&m.preview))
        .sum::<usize>()
        + width(CHIP_SEP) * (members.len() - 1);
    text <= room_for(indent_of(header), name_width(header), cells)
}

/// One entry, placed in its pane: which entry, and the lines it took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub entry: usize,
    /// Lines from the top of the list.
    pub y: usize,
    pub lines: usize,
}

/// Where each of `entries` lands in a list `room` lines tall.
///
/// The list starts at the top and scrolls only as far as it must to bring the
/// entry holding `selected` on screen — by whole entries, so no row is drawn
/// without the comment above it. Stateless, as the terminal's is: the frame
/// and a click resolved against it are worked out from the model alone, so
/// they cannot disagree. An entry that does not fit is left off, except the
/// selected one, which is clipped rather than lost.
pub fn place(entries: &[Entry<'_>], selected: Option<usize>, room: usize) -> Vec<Placed> {
    let heights: Vec<usize> = entries.iter().map(Entry::lines).collect();
    let selected = selected.and_then(|s| entries.iter().position(|e| e.holds(s)));

    let mut first = 0;
    if let Some(s) = selected {
        let mut span: usize = heights[..=s].iter().sum();
        while span > room && first < s {
            span -= heights[first];
            first += 1;
        }
    }

    let mut placed = Vec::new();
    let mut y = 0;
    for (entry, &h) in heights.iter().enumerate().skip(first) {
        if y + h > room {
            if Some(entry) == selected && y < room {
                placed.push(Placed {
                    entry,
                    y,
                    lines: room - y,
                });
            }
            break;
        }
        placed.push(Placed { entry, y, lines: h });
        y += h;
    }
    placed
}

/// Where the values of a list start: one column, just past the widest name,
/// so names read as one column and values as another — the file's
/// `key = value`, with the `=` taken out. Capped at half the width, so one
/// long key cannot push every value off the edge.
pub fn value_col(entries: &[Entry<'_>], cells: usize) -> usize {
    let widest = entries
        .iter()
        .filter(|e| !matches!(e, Entry::Caption { .. }))
        .map(|e| indent_of(e.item()) + name_width(e.item()))
        .max()
        .unwrap_or(0);
    (widest + 2).min(cells / 2)
}

// ── what an entry's lines say ────────────────────────────────────────────────

/// What a run of text is drawn as: a class in the editor's stylesheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Plain,
    Dim,
    Key,
    Value(VKind),
    Marker(Severity),
}

impl Ink {
    /// The class the stylesheet draws it with.
    pub fn class(self) -> &'static str {
        match self {
            Ink::Plain => "t",
            Ink::Dim => "t dim",
            Ink::Key => "t key",
            Ink::Value(VKind::Null) => "t v-null",
            Ink::Value(VKind::Bool) => "t v-bool",
            Ink::Value(VKind::Int | VKind::Float) => "t v-num",
            Ink::Value(VKind::Str) => "t v-str",
            Ink::Value(VKind::Ext) => "t v-ext",
            Ink::Value(VKind::Map | VKind::Seq) => "t dim",
            Ink::Marker(Severity::Error) => "t mark-error",
            Ink::Marker(Severity::Warning) => "t mark-warning",
            Ink::Marker(Severity::Info) => "t dim",
        }
    }
}

/// A run of one ink. `chip` is the item a click on it stands on, when it is
/// one of a chips row's members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub ink: Ink,
    pub chip: Option<usize>,
}

fn run(text: impl Into<String>, ink: Ink) -> Run {
    Run {
        text: text.into(),
        ink,
        chip: None,
    }
}

/// One line of an entry: its runs, laid end to end in cells.
pub type Line = Vec<Run>;

/// One entry as the lines it takes: a caption's gap, the first line of the
/// comment written above the item, then the line itself.
///
/// The comment takes one line whatever its length: a page is a list to scan,
/// and a paragraph between two of its rows would make it a page to read. The
/// whole block is a keystroke away (`C`).
pub fn lines_of(entry: &Entry<'_>, col: usize, cells: usize) -> Vec<Line> {
    let item = entry.item();
    let indent = indent_of(item);
    let mut lines = Vec::with_capacity(3);
    if matches!(entry, Entry::Caption { gap: true, .. }) {
        lines.push(Vec::new());
    }
    if let Some(comment) = &item.leading_comment {
        let first = comment.lines().next().unwrap_or_default();
        let text = fit(
            &format!("# {first}"),
            cells.saturating_sub(indent + RIGHT_GUTTER),
        );
        lines.push(vec![
            run(" ".repeat(indent), Ink::Plain),
            run(text, Ink::Dim),
        ]);
    }
    lines.push(match entry {
        Entry::Caption { item, .. } => caption_line(item, cells),
        Entry::Row { item, .. } => row_line(item, col, cells),
        Entry::Chips {
            header, members, ..
        } => chips_line(header, members, col, cells),
    });
    lines
}

/// A group's name over its members, bold, with the file's aside on it dimmed
/// after.
fn caption_line(item: &PageItem, cells: usize) -> Line {
    let indent = indent_of(item);
    let mut line = vec![run(" ".repeat(indent), Ink::Plain)];
    line.extend(name_runs(item));
    if let Some(comment) = &item.trailing_comment {
        let room = room_for(indent, name_width(item), cells).saturating_sub(2);
        let text = fit(&format!("# {comment}"), room);
        if width(&text) >= 4 {
            line.push(run("  ", Ink::Plain));
            line.push(run(text, Ink::Dim));
        }
    }
    line
}

/// One item as one row: the name on the left, its value or affordance in the
/// value column.
fn row_line(item: &PageItem, col: usize, cells: usize) -> Line {
    match &item.kind {
        ItemKind::Scalar => tail_line(item, &item.preview, Ink::Value(item.vkind), col, cells),
        ItemKind::GroupHeader { .. } => {
            let empty = if item.vkind == VKind::Seq { "[]" } else { "{}" };
            tail_line(item, empty, Ink::Dim, col, cells)
        }
        ItemKind::Drill { count } => {
            // What is in it when that fits, and a count when it does not.
            let trailing = match &item.summary {
                Some(flow)
                    if width(flow) + 2 <= room_for(indent_of(item), name_width(item), cells) =>
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
            tail_line(item, &trailing, Ink::Dim, col, cells)
        }
    }
}

/// A scalar sequence on one row: its name, then each member as a chip.
fn chips_line(header: &PageItem, members: &[(usize, &PageItem)], col: usize, cells: usize) -> Line {
    let indent = indent_of(header);
    let start = tail_start(indent, name_width(header), col);
    let end = cells.saturating_sub(RIGHT_GUTTER);
    let mut line = vec![run(" ".repeat(indent), Ink::Plain)];
    line.extend(name_runs(header));
    line.push(run(
        " ".repeat(start - indent - name_width(header)),
        Ink::Plain,
    ));
    let mut x = start;
    for (n, (index, member)) in members.iter().enumerate() {
        if n > 0 {
            let sep = fit(CHIP_SEP, end.saturating_sub(x));
            x += width(&sep);
            line.push(run(sep, Ink::Dim));
        }
        let text = fit(&member.preview, end.saturating_sub(x));
        if text.is_empty() {
            break;
        }
        x += width(&text);
        line.push(Run {
            text,
            ink: Ink::Value(member.vkind),
            chip: Some(*index),
        });
    }
    line
}

/// `indent + name   trailing  # comment`, the trailing text in the value
/// column and cut before the name ever is. The comment goes first when room
/// runs short, and is left off when fewer than a few characters would show.
fn tail_line(item: &PageItem, trailing: &str, ink: Ink, col: usize, cells: usize) -> Line {
    let indent = indent_of(item);
    let start = tail_start(indent, name_width(item), col);
    let room = cells.saturating_sub(start + RIGHT_GUTTER);
    let trailing = fit(trailing, room);
    let comment = item
        .trailing_comment
        .as_deref()
        .map(|c| fit(&format!("# {c}"), room.saturating_sub(width(&trailing) + 2)))
        .filter(|c| width(c) >= 4);

    let mut line = vec![run(" ".repeat(indent), Ink::Plain)];
    line.extend(name_runs(item));
    line.push(run(
        " ".repeat(start - indent - name_width(item)),
        Ink::Plain,
    ));
    line.push(run(trailing, ink));
    if let Some(comment) = comment {
        line.push(run("  ", Ink::Plain));
        line.push(run(comment, Ink::Dim));
    }
    line
}

/// What names an item on the left of its row: a host's finding first, then
/// the label — dimmed, before the title, when a sequence item has one.
fn name_runs(item: &PageItem) -> Vec<Run> {
    let mut runs = match &item.annotation {
        Some(a) => vec![run(
            format!("{} ", marker_glyph(a.severity)),
            Ink::Marker(a.severity),
        )],
        None => Vec::new(),
    };
    match &item.title {
        Some(title) => {
            runs.push(run(format!("{} ", item.label), Ink::Dim));
            runs.push(run(title.clone(), Ink::Key));
        }
        None => runs.push(run(item.label.clone(), Ink::Key)),
    }
    runs
}

/// One cell, always the same cell, whatever the severity, so the names do
/// not shift the moment a check runs.
pub fn marker_glyph(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "!",
        Severity::Warning => "?",
        Severity::Info => "·",
    }
}

fn indent_of(item: &PageItem) -> usize {
    2 + item.inset * 2
}

fn name_width(item: &PageItem) -> usize {
    name_runs(item).iter().map(|r| width(&r.text)).sum()
}

fn tail_start(indent: usize, name: usize, col: usize) -> usize {
    (indent + name + 2).max(col)
}

/// The cells left for a row's trailing text once its name has taken what it
/// needs.
fn room_for(indent: usize, name: usize, cells: usize) -> usize {
    cells.saturating_sub(indent + name + RIGHT_GUTTER + 2)
}

/// Cells, counted as characters: the type is monospace.
pub fn width(text: &str) -> usize {
    text.chars().count()
}

/// `text` cut to `room` cells, with an ellipsis where it was cut. Empty when
/// there is no room for even the ellipsis and one character.
pub fn fit(text: &str, room: usize) -> String {
    if width(text) <= room {
        text.to_string()
    } else if room >= 2 {
        text.chars().take(room - 1).chain(['…']).collect()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flower_core::{FigBackend, InlineBudget, Seg, ViewMode};

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

    fn text(line: &Line) -> String {
        line.iter().map(|r| r.text.as_str()).collect()
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
        m.focus_on(&[Seg::Key("server".into())]);
        m.page_enter();
        let p = panes(&m, true);
        let parent = p[0].as_ref().unwrap();
        assert_eq!(parent.role, Role::Parent);
        // The row the current page was opened through stays marked.
        let server = parent.page.items.iter().position(|i| i.label == "server");
        assert_eq!(parent.selected, server);
        assert_eq!(p[1].as_ref().unwrap().role, Role::Current);
    }

    #[test]
    fn values_line_up_in_one_column_and_comments_follow_them() {
        let mut m = model();
        m.focus_on(&[Seg::Key("server".into())]);
        m.page_enter();
        let page = m.page();
        let es = entries(page, 60);
        let col = value_col(&es, 60);
        let host = es.iter().find(|e| e.item().label == "host").unwrap();
        let port = es.iter().find(|e| e.item().label == "port").unwrap();
        let host = text(lines_of(host, col, 60).last().unwrap());
        let port = text(lines_of(port, col, 60).last().unwrap());
        assert_eq!(host.find("localhost"), port.find("8080"), "{host}\n{port}");
        assert!(host.ends_with("# dev only"), "{host}");
    }

    #[test]
    fn a_short_list_is_one_row_of_chips_each_its_own_item() {
        let mut m = model();
        m.focus_on(&[Seg::Key("server".into())]);
        m.page_enter();
        let es = entries(m.page(), 60);
        let chips = es
            .iter()
            .find(|e| matches!(e, Entry::Chips { .. }))
            .expect("a chips row");
        let line = lines_of(chips, value_col(&es, 60), 60).pop().unwrap();
        let on: Vec<_> = line
            .iter()
            .filter_map(|r| r.chip.map(|i| (i, r.text.clone())))
            .collect();
        assert_eq!(on.len(), 2);
        assert_eq!(on[0].1, "alpha");
        assert!(chips.holds(on[1].0));
        // Too narrow for both beside the name: a row per member instead.
        assert!(
            !entries(m.page(), 16)
                .iter()
                .any(|e| matches!(e, Entry::Chips { .. }))
        );
    }

    #[test]
    fn the_comment_above_an_item_takes_a_line_of_its_own() {
        let m = model();
        let es = entries(m.page(), 60);
        let server = es.iter().find(|e| e.item().label == "server").unwrap();
        let lines = lines_of(server, value_col(&es, 60), 60);
        assert_eq!(lines.len(), server.lines());
        assert!(text(&lines[lines.len() - 2]).contains("# the server block"));
    }

    #[test]
    fn the_list_scrolls_only_as_far_as_the_cursor_needs() {
        let src: String = (0..30).map(|i| format!("k{i} = {i}\n")).collect();
        let b = FigBackend::open(src.as_bytes(), fig_toml::format().unwrap()).unwrap();
        let mut m = Model::new(b).unwrap();
        m.set_view(ViewMode::Pages);
        let es = entries(m.page(), 40);
        let top = place(&es, Some(0), 10);
        assert_eq!(top.len(), 10);
        assert_eq!(top[0].entry, 0);
        let low = place(&es, Some(25), 10);
        assert_eq!(low.last().unwrap().entry, 25);
        assert_eq!(low.len(), 10);
    }

    #[test]
    fn a_long_value_is_cut_with_an_ellipsis_and_the_name_never_is() {
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("abc", 3), "abc");
        assert_eq!(fit("abc", 1), "");
        let b = FigBackend::open(
            b"name = \"a very long value that will not fit\"\n",
            fig_toml::format().unwrap(),
        )
        .unwrap();
        let m = Model::new(b).unwrap();
        let es = entries(m.page(), 20);
        let line = text(lines_of(&es[0], value_col(&es, 20), 20).last().unwrap());
        assert!(line.contains("name"), "{line}");
        assert!(line.ends_with('…'), "{line}");
        assert!(width(&line) <= 20);
    }
}
