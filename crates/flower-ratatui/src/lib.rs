//! A ratatui view over a [`flower_core::Model`]: a header line, the document
//! body, and a footer that doubles as the edit line.
//!
//! The embedding app owns the terminal and event loop; it calls [`draw`] each
//! frame and forwards key events to the model's methods. `header` is whatever
//! the app wants to name the document (e.g. a file name) — flower-core has no
//! filesystem concept of its own.
//!
//! The body renders whichever projection the model is in
//! ([`ViewMode`](flower_core::ViewMode)): the indented tree, or the page view —
//! a settings-menu layout that is two panes when there is width and depth to
//! justify them, and one pane when there isn't. That fallback is a rendering
//! decision, not a mode: the model holds one page state and this decides how much
//! of it fits.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use flower_core::{Backend, ItemKind, Mode, Model, Page, PageItem, VKind, ViewMode};

/// Below this width a two-pane split leaves neither pane usable, so the page view
/// collapses to the single-pane (push/pop) layout — the same interaction, one
/// column, which is all a narrow terminal or a phone ever had room for.
const TWO_PANE_MIN_WIDTH: u16 = 64;

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
pub fn draw<B: Backend>(f: &mut Frame, model: &Model<B>, header: &str) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Min(0),    // body
        Constraint::Length(1), // footer / edit line
    ])
    .split(f.area());

    draw_header(f, model, header, chunks[0]);
    match model.view() {
        ViewMode::Tree => draw_tree(f, model, chunks[1]),
        ViewMode::Pages => draw_pages(f, model, chunks[1]),
    }
    draw_footer(f, model, chunks[2]);
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

// ── the tree projection ──────────────────────────────────────────────────────

fn draw_tree<B: Backend>(f: &mut Frame, model: &Model<B>, area: Rect) {
    let items: Vec<ListItem> = model
        .rows
        .iter()
        .map(|row| {
            let indent = "  ".repeat(row.depth);

            // A twisty for containers, a bullet for leaves.
            let marker = if row.is_container() {
                if row.expanded { "▾ " } else { "▸ " }
            } else {
                "· "
            };

            let mut spans = vec![
                Span::raw(indent),
                Span::styled(marker, dim()),
                Span::styled(row.label.clone(), key_style()),
            ];

            if row.is_container() {
                spans.push(Span::styled(format!(" {}", row.preview), dim()));
            } else {
                spans.push(Span::raw(" = "));
                spans.push(Span::styled(row.preview.clone(), value_style(row.vkind)));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    render_list(f, items, Some(model.selected()), area);
}

// ── the page projection ──────────────────────────────────────────────────────

/// Two panes when the terminal is wide enough *and* the document has somewhere to
/// drill; one otherwise.
///
/// Both conditions matter. A flat document has no categories to put in a sidebar,
/// so splitting would spend half the width drawing an empty box next to the only
/// list there is.
fn draw_pages<B: Backend>(f: &mut Frame, model: &Model<B>, area: Rect) {
    if area.width < TWO_PANE_MIN_WIDTH || model.pages_would_degenerate() {
        draw_single_pane(f, model, area);
        return;
    }

    // Even halves. The two panes are consecutive levels of one lineage, not a
    // fixed index and a variable detail, so neither has a claim on more room than
    // the other — and the left is about to become the right.
    let cols =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

    // Exactly one pane owns the cursor: the left while you are still choosing on
    // the outer page, the right once you have opened something. That is what makes
    // `h` and `l` unambiguous without a focus-switch key.
    if model.focus().is_empty() {
        draw_page_pane(
            f,
            model.root_page(),
            Some(model.page_selected()),
            true,
            cols[0],
        );
        // Nothing has been opened yet, so the right pane previews what the cursor
        // would open. Without it the split would start half empty.
        match model.peek_page() {
            Some(peek) => draw_page_pane(f, &peek, None, false, cols[1]),
            None => draw_empty_detail(f, cols[1]),
        }
        return;
    }

    // A window sliding along the lineage: the left pane is the page the right one
    // was opened from, at every depth. It keeps the row you came out of marked, so
    // a deep page never loses the trace of what contains it.
    let parent = model.parent_page();
    let came_from = parent.position_of(model.focus());
    draw_page_pane(f, parent, came_from, false, cols[0]);
    draw_page_pane(f, model.page(), Some(model.page_selected()), true, cols[1]);
}

/// The narrow layout: just the page you are on, with the breadcrumb standing in
/// for the sidebar you don't have room for.
fn draw_single_pane<B: Backend>(f: &mut Frame, model: &Model<B>, area: Rect) {
    draw_page_pane(f, model.page(), Some(model.page_selected()), true, area);
}

/// One page: a breadcrumb line, then its items. `selected` is `None` for a pane
/// that is only being previewed; `active` dims the breadcrumb of a pane that does
/// not hold the cursor.
fn draw_page_pane(f: &mut Frame, page: &Page, selected: Option<usize>, active: bool, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);

    let crumb = page.breadcrumb(ROOT_LABEL);
    let style = if active {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        dim()
    };
    f.render_widget(
        Line::from(Span::styled(format!(" {crumb}"), style)),
        rows[0],
    );

    if page.is_empty() {
        f.render_widget(Line::from(Span::styled("   (empty)", dim())), rows[1]);
        return;
    }

    let width = rows[1].width;
    let items: Vec<ListItem> = page
        .items
        .iter()
        .map(|item| ListItem::new(page_line(item, width)))
        .collect();
    render_list(f, items, selected, rows[1]);
}

fn draw_empty_detail(f: &mut Frame, area: Rect) {
    f.render_widget(Line::from(Span::styled("  select a section", dim())), area);
}

/// One page item, laid out the way a settings row reads: the name on the left,
/// its value or affordance flushed right.
///
/// Right-aligning the values is what makes a page scannable — the names form one
/// column and the values another, instead of a ragged `key = value` edge that
/// moves with every key length.
fn page_line(item: &PageItem, width: u16) -> Line<'static> {
    let indent = 2 + item.inset * 2;

    match &item.kind {
        // A rule with the group's name in it: enough to bind the members below it
        // together, without a second indentation scheme to read.
        //
        // No drill chevron, though the header does open a page: at the right-hand
        // end of a rule it reads as part of the rule, and the affordance is not
        // worth the noise on every group — the members it would lead to are
        // already on screen, which is the whole point of inlining them.
        ItemKind::GroupHeader { .. } => {
            let name = match &item.title {
                Some(title) => format!("{} · {title}", item.label),
                None => item.label.clone(),
            };
            let head = format!("{}── {name} ", " ".repeat(indent));
            let rule = (width as usize).saturating_sub(head.chars().count() + 1);
            Line::from(vec![
                Span::styled(head, dim()),
                Span::styled("─".repeat(rule), dim()),
            ])
        }
        ItemKind::Drill { count } => {
            let name = name_spans(item);
            // Show what is in it when what is in it fits: `1 field ›` is strictly
            // less than the document says when the field is right there. The count
            // is the fallback for a container too big to put on the row, which is
            // the only case where counting beats showing.
            let trailing = match &item.summary {
                Some(flow) if flow.chars().count() + 2 <= room_for(indent, &name, width) => {
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
            row_line(indent, name, &trailing, dim(), width)
        }
        ItemKind::Scalar => row_line(
            indent,
            name_spans(item),
            &item.preview,
            value_style(item.vkind),
            width,
        ),
    }
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

/// `indent + name … trailing`, with `trailing` flushed to `width` and truncated
/// before the name is ever squeezed — a name you can't read costs more than a
/// value you can't finish.
fn row_line(
    indent: usize,
    name: Vec<Span<'static>>,
    trailing: &str,
    trailing_style: Style,
    width: u16,
) -> Line<'static> {
    let width = width as usize;
    let name_w: usize = name.iter().map(|s| s.content.chars().count()).sum();
    let room = room_for(indent, &name, width as u16);

    let trailing: String = if trailing.chars().count() <= room {
        trailing.to_string()
    } else if room >= 2 {
        trailing.chars().take(room - 1).chain(['…']).collect()
    } else {
        String::new()
    };

    let pad = width
        .saturating_sub(indent + name_w + trailing.chars().count() + RIGHT_GUTTER)
        .max(1);

    let mut spans = vec![Span::raw(" ".repeat(indent))];
    spans.extend(name);
    spans.push(Span::raw(" ".repeat(pad)));
    spans.push(Span::styled(trailing, trailing_style));
    Line::from(spans)
}

/// How many columns are left for a row's trailing text once its name has taken
/// what it needs — the name is never squeezed, because a name you can't read
/// costs more than a value you can't finish.
fn room_for(indent: usize, name: &[Span<'static>], width: u16) -> usize {
    let name_w: usize = name.iter().map(|s| s.content.chars().count()).sum();
    (width as usize).saturating_sub(indent + name_w + RIGHT_GUTTER + 1)
}

fn render_list(f: &mut Frame, items: Vec<ListItem>, selected: Option<usize>, area: Rect) {
    let empty = items.is_empty();
    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 40, 55))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");

    let mut state = ListState::default();
    if !empty {
        state.select(selected);
    }
    f.render_stateful_widget(list, area, &mut state);
}

// ── footer ───────────────────────────────────────────────────────────────────

fn draw_footer<B: Backend>(f: &mut Frame, model: &Model<B>, area: Rect) {
    // Kept short enough to survive an 80-column terminal alongside the status.
    let hints = match model.view() {
        ViewMode::Tree => "  j/k · h/l fold · Enter edit · x del · v pages · s save · q quit",
        ViewMode::Pages => "  j/k · l/h in/out · e edit · x del · v tree · s save · q quit",
    };
    let line = match &model.mode {
        Mode::Editing { buffer, .. } => Line::from(vec![
            Span::styled(
                " edit ",
                Style::default().bg(Color::Yellow).fg(Color::Black),
            ),
            Span::raw(" "),
            Span::raw(buffer.clone()),
            Span::styled("▏", Style::default().fg(Color::Yellow)),
            Span::styled("   (Enter to commit · Esc to cancel)", dim()),
        ]),
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
    use flower_core::{FigBackend, Seg};
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
        // Values are flushed right, not written as `key = value`.
        assert!(!out.contains("host = "), "{out}");
    }

    #[test]
    fn a_group_is_inlined_under_a_titled_rule() {
        let mut model = model();
        model.focus_on(&[Seg::Key("server".into())]);
        model.page_enter();
        let out = render(&model, 76, 12);
        assert!(out.contains("── limits "), "{out}");
        assert!(out.contains("── tags "), "{out}");
        assert!(out.contains("server"), "{out}");
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

    #[test]
    fn a_small_container_shows_its_contents_on_the_row() {
        let backend = FigBackend::open(
            br#"{"on": {"push": {"branches": ["master"]}, "jobs": {"a": {"b": {"c": 1}}}}}"#,
            fig::Format::Json,
        )
        .expect("open");
        let mut m = Model::new(backend).expect("model");
        m.set_view(ViewMode::Pages);
        m.focus_on(&[Seg::Key("on".into())]);
        m.page_enter();
        let out = render(&m, 100, 10);
        assert!(out.contains("{branches: [master]} ›"), "{out}");
        assert!(!out.contains("1 field ›"), "{out}");
    }
}
