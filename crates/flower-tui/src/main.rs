//! flower — a structural TUI editor for config files, built on flower-core.
//!
//! This binary owns the two things flower-core deliberately does not: the file
//! (read on open, written on save) and the terminal event loop.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

use flower_core::{FigBackend, Mode, Model};

fn main() -> Result<()> {
    let path = match std::env::args_os().nth(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: flower <config-file>");
            std::process::exit(2);
        }
    };

    let fmt = flower_core::detect(&path).with_context(|| {
        format!(
            "unrecognized config extension for {} (want json/yaml/toml/zon/fig)",
            path.display()
        )
    })?;

    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let backend = FigBackend::open(&bytes, fmt)?;
    let mut model = Model::new(backend)?;

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut model, &path, fmt);
    ratatui::restore();
    result
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    model: &mut Model<FigBackend>,
    path: &Path,
    fmt: fig::Format,
) -> Result<()> {
    let file = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let name = format!("{file}  [{fmt:?}]");

    loop {
        terminal.draw(|f| flower_ratatui::draw(f, model, &name))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match &model.mode {
            Mode::Normal => {
                if handle_normal(model, key.code, path) {
                    return Ok(());
                }
            }
            Mode::Editing { .. } => handle_editing(model, key.code),
        }
    }
}

/// Returns `true` when the app should quit.
fn handle_normal(model: &mut Model<FigBackend>, code: KeyCode, path: &Path) -> bool {
    match code {
        KeyCode::Char('q') => return true,
        KeyCode::Char('j') | KeyCode::Down => model.move_down(),
        KeyCode::Char('k') | KeyCode::Up => model.move_up(),
        KeyCode::Char('l') | KeyCode::Right => model.expand_or_enter(),
        KeyCode::Char('h') | KeyCode::Left => model.collapse_or_leave(),
        KeyCode::Enter | KeyCode::Char(' ') => model.activate(),
        KeyCode::Char('e') => model.begin_edit(),
        KeyCode::Char('x') => model.delete_selected(),
        KeyCode::Char('s') => save(model, path),
        _ => {}
    }
    false
}

fn handle_editing(model: &mut Model<FigBackend>, code: KeyCode) {
    match code {
        KeyCode::Char(c) => model.edit_push(c),
        KeyCode::Backspace => model.edit_backspace(),
        KeyCode::Enter => model.edit_commit(),
        KeyCode::Esc => model.edit_cancel(),
        _ => {}
    }
}

fn save(model: &mut Model<FigBackend>, path: &Path) {
    match std::fs::write(path, model.source_snapshot()) {
        Ok(()) => {
            model.mark_saved();
            let name = path.file_name().map(|s| s.to_string_lossy().into_owned());
            model.set_status(format!("saved {}", name.as_deref().unwrap_or("file")));
        }
        Err(e) => model.set_status(format!("save failed: {e}")),
    }
}
