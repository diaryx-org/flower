//! ratatui rendering: a header line, the tree body, and a footer that doubles as
//! the edit line.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::app::{App, Mode};
use crate::tree::VKind;

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

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Min(0),    // tree
        Constraint::Length(1), // footer / edit line
    ])
    .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_tree(f, app, chunks[1]);
    draw_footer(f, app, chunks[2]);
}

fn draw_header(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let dirty = if app.dirty { " ●" } else { "" };
    let title = format!(
        " flower — {}{}  [{:?}]",
        app.file_name(),
        dirty,
        app.format()
    );
    let line = Line::from(Span::styled(
        title,
        Style::default()
            .fg(Color::Black)
            .bg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));
    f.render_widget(line, area);
}

fn draw_tree(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let items: Vec<ListItem> = app
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
                Span::styled(marker, Style::default().fg(Color::DarkGray)),
                Span::styled(
                    row.label.clone(),
                    Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                ),
            ];

            if row.is_container() {
                spans.push(Span::styled(
                    format!(" {}", row.preview),
                    Style::default().fg(Color::DarkGray),
                ));
            } else {
                spans.push(Span::raw(" = "));
                spans.push(Span::styled(row.preview.clone(), value_style(row.vkind)));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 40, 55))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");

    let mut state = ListState::default();
    if !app.rows.is_empty() {
        state.select(Some(app.selected));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_footer(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let line = match &app.mode {
        Mode::Editing { buffer } => Line::from(vec![
            Span::styled(
                " edit ",
                Style::default().bg(Color::Yellow).fg(Color::Black),
            ),
            Span::raw(" "),
            Span::raw(buffer.clone()),
            Span::styled("▏", Style::default().fg(Color::Yellow)),
            Span::styled(
                "   (Enter to commit · Esc to cancel)",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Mode::Normal => Line::from(vec![
            Span::styled(
                format!(" {} ", app.status),
                Style::default().fg(Color::Black).bg(Color::Green),
            ),
            Span::styled(
                "  j/k move · h/l fold · Enter edit · x del · s save · q quit",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    };
    f.render_widget(line, area);
}
