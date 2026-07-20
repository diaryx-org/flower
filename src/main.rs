//! flower — a structural TUI editor for config files, built on fig.

mod app;
mod format;
mod tree;
mod ui;

use std::path::PathBuf;

use anyhow::{Context, Result};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

use app::{App, Mode};

fn main() -> Result<()> {
    let path = match std::env::args_os().nth(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: flower <config-file>");
            std::process::exit(2);
        }
    };

    let fmt = format::detect(&path).with_context(|| {
        format!(
            "unrecognized config extension for {} (want json/yaml/toml/zon/fig)",
            path.display()
        )
    })?;

    let mut app = App::open(path, fmt)?;

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match &app.mode {
            Mode::Normal => handle_normal(app, key.code),
            Mode::Editing { .. } => handle_editing(app, key.code),
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_normal(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.move_up(),
        KeyCode::Char('l') | KeyCode::Right => app.expand_or_enter(),
        KeyCode::Char('h') | KeyCode::Left => app.collapse_or_leave(),
        KeyCode::Enter | KeyCode::Char(' ') => app.activate(),
        KeyCode::Char('e') => app.begin_edit(),
        KeyCode::Char('x') => app.delete_selected(),
        KeyCode::Char('s') => app.save(),
        _ => {}
    }
}

fn handle_editing(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char(c) => app.edit_push(c),
        KeyCode::Backspace => app.edit_backspace(),
        KeyCode::Enter => app.edit_commit(),
        KeyCode::Esc => app.edit_cancel(),
        _ => {}
    }
}
