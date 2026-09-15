//! flower — a structural TUI editor for config files, built on flower-core.
//!
//! This binary owns the two things flower-core deliberately does not: the file
//! (read on open, written on save) and the terminal event loop. The keys and
//! the mouse themselves belong to `flower-ratatui` — this host forwards an
//! event and acts on the [`flower_ratatui::Outcome`] that comes back, so an app
//! embedding the widget gets exactly the interaction this binary has.

use std::io::stdout;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::Rect;

use flower_core::{FigBackend, Model, ViewMode};
use flower_ratatui::Outcome;

/// The one-line usage, shared by the no-argument error and `--help` so the two
/// can never drift apart.
const USAGE: &str = "usage: flower <config-file>";

fn main() -> Result<()> {
    let arg = match std::env::args_os().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };

    // `--version` and `--help` are answered before the file is read or the
    // terminal is entered. Homebrew's formula test is `flower --version` on a
    // machine with no config file to hand, and every argument below this point
    // is treated as a path — so a flag that fell through would be rejected by
    // `detect` as an unrecognized extension and exit non-zero. Printing the
    // crate version is also what makes that test meaningful: it is what `brew`
    // matches the formula's version against, which catches a mis-tagged release.
    if let Some(flag) = arg.to_str() {
        match flag {
            "--version" | "-V" => {
                println!("flower {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => {}
        }
    }

    let path = PathBuf::from(arg);

    let fmt = flower_core::detect(&path).with_context(|| {
        format!(
            "unrecognized config extension for {} (want json/yaml/toml/zon/fig)",
            path.display()
        )
    })?;

    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let backend = FigBackend::open(&bytes, fmt)?;
    let mut model = Model::new(backend)?;
    // The page projection is the only one this app draws. The model routes the
    // node keys (`e`, `x`) through whichever view is active, so say it once here
    // rather than leaving the first edit before any movement to resolve against
    // a tree cursor nothing on screen came from.
    model.set_view(ViewMode::Pages);

    let mut terminal = ratatui::init();
    // Best effort, like `restore` is: a terminal that cannot report the mouse
    // still has the keys, and the widget ignores what never arrives.
    let _ = execute!(stdout(), EnableMouseCapture);
    let result = run(&mut terminal, &mut model, &path, fmt);
    let _ = execute!(stdout(), DisableMouseCapture);
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

    // How much a page inlines is a fact about the room, so the room is measured
    // before the first frame rather than after it — and again on every frame,
    // because a terminal can be resized under us and the model rebuilds only
    // when the answer actually moves.
    fit(model, terminal)?;
    // Only now: where the document opens depends on what the budget put on the
    // root page. A document small enough to fit entirely has no lone drill row
    // to start past, and asking before the budget was known would have found one.
    model.enter_document();

    loop {
        fit(model, terminal)?;
        terminal.draw(|f| flower_ratatui::draw(f, model, &name))?;

        let outcome = match event::read()? {
            // A Windows console reports releases and repeats as well as
            // presses, and the widget takes whatever it is given — so the
            // filter is here, where the events are read.
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                flower_ratatui::handle_key(model, key)
            }
            // The editor is the whole frame, so the widget resolves the click
            // against the terminal's own rectangle.
            Event::Mouse(mouse) => {
                let size = terminal.size().context("terminal size")?;
                let area = Rect::new(0, 0, size.width, size.height);
                flower_ratatui::handle_mouse(model, area, mouse)
            }
            _ => continue,
        };

        match outcome {
            Outcome::Continue => {}
            Outcome::Quit => return Ok(()),
            Outcome::Save => save(model, path),
        }
    }
}

/// Size the model's inline budget to the terminal it is being drawn in.
fn fit(model: &mut Model<FigBackend>, terminal: &ratatui::DefaultTerminal) -> Result<()> {
    let height = terminal.size().context("terminal size")?.height;
    model.fit_to_room(flower_ratatui::page_room(height));
    Ok(())
}

/// [`Outcome::Save`], performed: the file is this binary's, not the widget's.
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
