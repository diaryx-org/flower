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

/// What the sidebar takes when there are two panes.
const SIDEBAR_PCT: u16 = 34;
const SIDEBAR_MIN: u16 = 22;
const SIDEBAR_MAX: u16 = 38;

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

    render_list(f, items, Some(model.selected), area);
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

    let sidebar = (area.width * SIDEBAR_PCT / 100).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
    let cols = Layout::horizontal([Constraint::Length(sidebar), Constraint::Min(0)]).split(area);

    // Exactly one pane owns the cursor: the sidebar while you are still choosing a
    // category, the detail pane once you have opened one. That is what makes `h`
    // and `l` unambiguous without a focus-switch key.
    let at_root = model.focus().is_empty();
    let root = model.root_page();

    // The sidebar keeps the branch you are inside marked even though the cursor
    // has moved on to the detail pane — without it, a deep page loses all trace of
    // which category it belongs to.
    let lineage = (!at_root)
        .then(|| model.focus().first().cloned())
        .flatten()
        .and_then(|seg| root.position_of(std::slice::from_ref(&seg)));

    draw_page_pane(
        f,
        root,
        if at_root {
            Some(model.page_selected())
        } else {
            lineage
        },
        at_root,
        ROOT_LABEL,
        cols[0],
    );

    // At the root the detail pane previews what the cursor is about to open, so
    // the split is never half empty.
    let peeked = at_root.then(|| model.peek_page()).flatten();
    match (at_root, peeked) {
        (true, Some(peek)) => draw_page_pane(f, &peek, None, false, ROOT_LABEL, cols[1]),
        (true, None) => draw_empty_detail(f, cols[1]),
        (false, _) => draw_page_pane(
            f,
            model.page(),
            Some(model.page_selected()),
            true,
            ROOT_LABEL,
            cols[1],
        ),
    }
}

/// The narrow layout: just the page you are on, with the breadcrumb standing in
/// for the sidebar you don't have room for.
fn draw_single_pane<B: Backend>(f: &mut Frame, model: &Model<B>, area: Rect) {
    draw_page_pane(
        f,
        model.page(),
        Some(model.page_selected()),
        true,
        ROOT_LABEL,
        area,
    );
}

/// One page: a breadcrumb line, then its items. `selected` is `None` for a pane
/// that is only being previewed; `active` dims the breadcrumb of a pane that does
/// not hold the cursor.
fn draw_page_pane(
    f: &mut Frame,
    page: &Page,
    selected: Option<usize>,
    active: bool,
    root_label: &str,
    area: Rect,
) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);

    let crumb = page.breadcrumb(root_label);
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
            let title = format!("{}── {} ", " ".repeat(indent), item.label);
            let rule = (width as usize).saturating_sub(title.chars().count() + 1);
            Line::from(vec![
                Span::styled(title, dim()),
                Span::styled("─".repeat(rule), dim()),
            ])
        }
        ItemKind::Drill { count } => {
            let noun = if item.vkind == VKind::Seq {
                "items"
            } else {
                "fields"
            };
            let trailing = format!("{count} {noun} ›");
            row_line(indent, &item.label, key_style(), &trailing, dim(), width)
        }
        ItemKind::Scalar => row_line(
            indent,
            &item.label,
            key_style(),
            &item.preview,
            value_style(item.vkind),
            width,
        ),
    }
}

/// `indent + label … trailing`, with `trailing` flushed to `width` and truncated
/// before the label is ever squeezed — a name you can't read costs more than a
/// value you can't finish.
fn row_line(
    indent: usize,
    label: &str,
    label_style: Style,
    trailing: &str,
    trailing_style: Style,
    width: u16,
) -> Line<'static> {
    const GUTTER: usize = 1; // kept clear on the right so a value never touches the edge
    let width = width as usize;
    let label_w = label.chars().count();
    let room = width.saturating_sub(indent + label_w + GUTTER + 1);

    let trailing: String = if trailing.chars().count() <= room {
        trailing.to_string()
    } else if room >= 2 {
        trailing.chars().take(room - 1).chain(['…']).collect()
    } else {
        String::new()
    };

    let pad = width
        .saturating_sub(indent + label_w + trailing.chars().count() + GUTTER)
        .max(1);

    Line::from(vec![
        Span::raw(" ".repeat(indent)),
        Span::styled(label.to_string(), label_style),
        Span::raw(" ".repeat(pad)),
        Span::styled(trailing, trailing_style),
    ])
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
        Mode::Normal => Line::from(vec![
            Span::styled(
                format!(" {} ", model.status),
                Style::default().fg(Color::Black).bg(Color::Green),
            ),
            Span::styled(hints, dim()),
        ]),
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
}
