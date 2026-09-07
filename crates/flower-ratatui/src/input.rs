//! The keyboard half of the widget: the key table that drives a
//! [`flower_core::Model`], and the [`Outcome`] naming what the *host* must do
//! about a key the widget deliberately does not act on itself.
//!
//! Everything that only moves the cursor or edits the document happens here, in
//! the crate that draws it — so an app embedding flower as a pane gets flower's
//! keys by forwarding an event, not by copying a match arm out of `flower-tui`.
//! Quitting and saving are the host's: this crate has no terminal to leave and
//! no file to write (flower-core is filesystem-free by design), so they come
//! back as [`Outcome::Quit`] and [`Outcome::Save`] for the host to answer with
//! its own unsaved-changes handling, its own path, and its own error reporting.

use flower_core::{Backend, Mode, Model};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

/// What the host must do after the widget has handled a key.
///
/// The two non-`Continue` variants are the things a widget cannot do for
/// itself: it owns a `Model`, not a terminal and not a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Handled by the widget (or ignored). Redraw and read the next event.
    Continue,
    /// `q` — the host exits (guarding an unsaved document as it sees fit).
    Quit,
    /// `s` — the host writes [`Model::source_snapshot`] wherever the document
    /// came from, then tells the model how it went: [`Model::mark_saved`] and a
    /// [`Model::set_status`] on success, a status alone on failure.
    Save,
}

/// Apply what `key` implies to `model`, returning the [`Outcome`] the host must
/// act on.
///
/// Both modes are here. In [`Mode::Normal`] this is one flat table over the page
/// projection — `j`/`k` walk the page, `l`/`h` push and pop one, and the keys
/// that operate on a *node* (`e` edit, `c`/`C` its trailing / leading comment,
/// `x` delete) resolve against the page cursor. In [`Mode::Editing`] every
/// printable character goes into the buffer, `Enter` commits and `Esc` cancels;
/// nothing returns an outcome, since an edit in progress is entirely the
/// model's business.
///
/// Only the key *code* is read, so a modifier a terminal happens to attach does
/// not stop a binding from firing. The host is expected to filter to
/// `KeyEventKind::Press` (a Windows console reports releases too) and to
/// intercept its own overlays — a dialog, a command palette — before forwarding
/// here.
pub fn handle_key<B: Backend>(model: &mut Model<B>, key: KeyEvent) -> Outcome {
    // Matched into a bool rather than borrowed across the call: the arms take
    // `&mut Model`, and `Mode::Editing` carries a buffer the borrow checker is
    // right to protect.
    if matches!(model.mode, Mode::Editing { .. }) {
        editing(model, key.code);
        return Outcome::Continue;
    }
    normal(model, key.code)
}

/// The normal-mode table. See [`handle_key`] for why the modifiers are ignored.
fn normal<B: Backend>(model: &mut Model<B>, code: KeyCode) -> Outcome {
    match code {
        KeyCode::Char('q') => return Outcome::Quit,
        KeyCode::Char('s') => return Outcome::Save,
        KeyCode::Char('e') => model.begin_edit(),
        KeyCode::Char('c') => model.begin_edit_trailing_comment(),
        KeyCode::Char('C') => model.begin_edit_leading_comment(),
        KeyCode::Char('x') => model.delete_selected(),
        KeyCode::Char('j') | KeyCode::Down => model.page_move_down(),
        KeyCode::Char('k') | KeyCode::Up => model.page_move_up(),
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => {
            model.page_enter()
        }
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => model.page_back(),
        _ => {}
    }
    Outcome::Continue
}

/// Typing into the value field the footer doubles as.
fn editing<B: Backend>(model: &mut Model<B>, code: KeyCode) {
    match code {
        KeyCode::Char(c) => model.edit_push(c),
        KeyCode::Backspace => model.edit_backspace(),
        KeyCode::Enter => model.edit_commit(),
        KeyCode::Esc => model.edit_cancel(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flower_core::{FigBackend, Seg, ViewMode};
    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};

    const SAMPLE: &str = "\
title = \"flower\"
version = 1

[server]
host = \"localhost\"
port = 8080

[server.limits]
max_connections = 100
";

    fn model() -> Model<FigBackend> {
        let backend = FigBackend::open(SAMPLE.as_bytes(), fig::Format::Toml).expect("open");
        let mut model = Model::new(backend).expect("model");
        model.set_view(ViewMode::Pages);
        model
    }

    fn press<B: Backend>(model: &mut Model<B>, code: KeyCode) -> Outcome {
        handle_key(model, KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn quit_and_save_are_the_hosts_to_perform() {
        let mut m = model();
        assert_eq!(press(&mut m, KeyCode::Char('q')), Outcome::Quit);
        assert_eq!(press(&mut m, KeyCode::Char('s')), Outcome::Save);
        // Neither touched the document — the host is what acts on them.
        assert!(!m.dirty);
    }

    #[test]
    fn c_and_shift_c_open_the_two_comments_in_the_footer() {
        let mut m = model();
        m.focus_on(&[Seg::Key("version".into())]);
        assert_eq!(press(&mut m, KeyCode::Char('c')), Outcome::Continue);
        assert!(matches!(
            m.mode,
            Mode::Editing {
                slot: flower_core::EditSlot::TrailingComment,
                ..
            }
        ));
        for c in "bump me".chars() {
            press(&mut m, KeyCode::Char(c));
        }
        press(&mut m, KeyCode::Enter);
        assert!(m.source_snapshot().contains("version = 1 # bump me"));

        press(&mut m, KeyCode::Char('C'));
        assert!(matches!(
            m.mode,
            Mode::Editing {
                slot: flower_core::EditSlot::LeadingComment,
                ..
            }
        ));
        press(&mut m, KeyCode::Esc);
        assert!(matches!(m.mode, Mode::Normal));
    }

    #[test]
    fn the_movement_keys_walk_the_page() {
        let mut m = model();
        let start = m.page_selected();
        assert_eq!(press(&mut m, KeyCode::Char('j')), Outcome::Continue);
        assert_eq!(m.page_selected(), start + 1);
        press(&mut m, KeyCode::Up);
        assert_eq!(m.page_selected(), start);
    }

    #[test]
    fn l_and_h_push_and_pop_a_page() {
        let mut m = model();
        // A room too short to inline `server` into the root page, so there is a
        // page to push and pop rather than a group already on screen.
        m.fit_to_room(crate::page_room(9));
        m.focus_on(&[Seg::Key("server".into())]);
        assert!(m.focus().is_empty());
        press(&mut m, KeyCode::Char('l'));
        assert_eq!(m.focus(), [Seg::Key("server".into())]);
        press(&mut m, KeyCode::Char('h'));
        assert!(m.focus().is_empty());
    }

    #[test]
    fn e_opens_the_edit_line_and_typing_commits_through_it() {
        let mut m = model();
        m.focus_on(&[Seg::Key("server".into()), Seg::Key("host".into())]);
        assert_eq!(press(&mut m, KeyCode::Char('e')), Outcome::Continue);
        assert!(matches!(m.mode, Mode::Editing { .. }));

        // In edit mode the same letters are text, not commands: `q` types a `q`
        // rather than asking the host to quit.
        for c in "qs".chars() {
            assert_eq!(press(&mut m, KeyCode::Char(c)), Outcome::Continue);
        }
        press(&mut m, KeyCode::Backspace);
        press(&mut m, KeyCode::Enter);
        assert!(matches!(m.mode, Mode::Normal));
        assert!(m.source_snapshot().contains('q'), "{}", m.source_snapshot());
    }

    #[test]
    fn esc_cancels_an_edit_without_touching_the_document() {
        let mut m = model();
        m.focus_on(&[Seg::Key("title".into())]);
        press(&mut m, KeyCode::Char('e'));
        press(&mut m, KeyCode::Char('z'));
        press(&mut m, KeyCode::Esc);
        assert!(matches!(m.mode, Mode::Normal));
        assert!(!m.dirty);
        assert_eq!(m.source_snapshot(), SAMPLE);
    }
}
