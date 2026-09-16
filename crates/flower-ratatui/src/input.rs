//! The input half of the widget: the key table and the mouse gestures that
//! drive a [`flower_core::Model`], and the [`Outcome`] naming what the *host*
//! must do about an event the widget deliberately does not act on itself.
//!
//! Everything that only moves the cursor or edits the document happens here, in
//! the crate that draws it — so an app embedding flower as a pane gets flower's
//! keys by forwarding an event, not by copying a match arm out of `flower-tui`,
//! and its clicks by forwarding the event with the `Rect` it drew into, not by
//! restating where the rows went. Quitting and saving are the host's: this
//! crate has no terminal to leave and no file to write (flower-core is
//! filesystem-free by design), so they come back as [`Outcome::Quit`] and
//! [`Outcome::Save`] for the host to answer with its own unsaved-changes
//! handling, its own path, and its own error reporting.

use flower_core::{Backend, Mode, Model};
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use crate::{Hit, hit_at};

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
/// `x` delete) resolve against the page cursor, while `u`/`U` undo and redo
/// whatever the document's own journal last recorded, wherever the cursor is. In [`Mode::Editing`] every
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
    if matches!(model.mode, Mode::Choosing { .. }) {
        choosing(model, key.code);
        return Outcome::Continue;
    }
    normal(model, key.code)
}

/// The normal-mode table. See [`handle_key`] for why the modifiers are ignored.
fn normal<B: Backend>(model: &mut Model<B>, code: KeyCode) -> Outcome {
    match code {
        KeyCode::Char('q') => return Outcome::Quit,
        KeyCode::Char('s') => return Outcome::Save,
        // The picker where the field has one, the text field where it does not
        // — one key, because `begin_choose` falls back
        // ([`Model::begin_choose`](flower_core::Model::begin_choose)). `E` is
        // the way past it: a value typed by hand goes through the same
        // validation a chosen one would, and an open vocabulary is a list of
        // suggestions rather than the whole of what is legal.
        KeyCode::Char('e') => model.begin_choose(),
        KeyCode::Char('E') => model.begin_edit(),
        KeyCode::Char('c') => model.begin_edit_trailing_comment(),
        KeyCode::Char('C') => model.begin_edit_leading_comment(),
        KeyCode::Char('x') => model.delete_selected(),
        // vi's `u`, and `U` for the way back — the pair flower-core journals
        // ([`Model::undo`](flower_core::Model::undo)). A save is not a
        // boundary: `u` runs back through one.
        KeyCode::Char('u') => model.undo(),
        KeyCode::Char('U') => model.redo(),
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

/// Apply what `mouse` implies to `model`, drawn into `area` — the `Rect` the
/// host handed [`draw_in`](crate::draw_in), or the whole frame for
/// [`draw`](crate::draw).
///
/// A click stands on the row it lands on. A second click on the row the cursor
/// is already on is Enter — a container opens as a page, a value opens for
/// editing. Two clicks rather than a double-click, because a terminal reports
/// no such thing and a timing guess would make a slow second click a different
/// gesture from a quick one. In the two-pane layout the other pane is one step
/// along the lineage in one direction or the other, and a click there takes
/// that step first: the left pane is the page this one was opened from, so a
/// click backs out onto the row clicked; the right is a preview of what the
/// cursor would open, so a click opens it onto the row clicked. The wheel
/// walks the cursor, wherever over the editor it turns.
///
/// An event outside `area` is not the widget's and is ignored, which is what
/// lets a host forward every mouse event unread. So is any event while a value
/// is open: flower's own keys leave the cursor alone while editing, and a click
/// that moved it out from under a half-typed value would commit that value to
/// the wrong row.
///
/// Returns an [`Outcome`] for the same reason [`handle_key`] does. No gesture
/// asks anything of the host yet, so today it is always
/// [`Outcome::Continue`].
pub fn handle_mouse<B: Backend>(model: &mut Model<B>, area: Rect, mouse: MouseEvent) -> Outcome {
    let at = Position::new(mouse.column, mouse.row);
    if !area.contains(at) || matches!(model.mode, Mode::Editing { .. } | Mode::Choosing { .. }) {
        return Outcome::Continue;
    }
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(hit) = hit_at(model, area, at) {
                click(model, hit);
            }
        }
        MouseEventKind::ScrollDown => model.page_move_down(),
        MouseEventKind::ScrollUp => model.page_move_up(),
        _ => {}
    }
    Outcome::Continue
}

/// What a click on a row does, in the page vocabulary the keys use.
fn click<B: Backend>(model: &mut Model<B>, hit: Hit) {
    match hit {
        Hit::Row(i) if i == model.page_selected() => model.page_enter(),
        Hit::Row(i) => model.page_select(i),
        Hit::ParentRow(i) => {
            model.page_back();
            model.page_select(i);
        }
        // Only a container has a preview, so this is a page and never an edit.
        Hit::PeekRow(i) => {
            model.page_enter();
            model.page_select(i);
        }
    }
}

/// Driving the picker: the same `j`/`k` the page takes, arrows for a reader
/// who is typing, everything else printable narrowing the list.
///
/// `j` and `k` are movement here and not filter text, which is the one
/// asymmetry with the edit line — a vi-shaped list that could not be walked
/// with `j` would be a list nobody could walk without reaching for the arrows,
/// and a filter is reached for by typing a word, not by typing one letter.
fn choosing<B: Backend>(model: &mut Model<B>, code: KeyCode) {
    match code {
        KeyCode::Char('j') | KeyCode::Down => model.choose_next(),
        KeyCode::Char('k') | KeyCode::Up => model.choose_prev(),
        KeyCode::Char(c) => model.choose_push(c),
        KeyCode::Backspace => model.choose_backspace(),
        KeyCode::Enter => model.choose_commit(),
        KeyCode::Esc => model.choose_cancel(),
        _ => {}
    }
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
        // The founding budget, one rank deep: `server` holds a group of its
        // own, so it drills, and there is a page to push and pop rather than a
        // group already on screen. Fitted to a room it would inline — a third
        // of any room is at least six rows, and two ranks are admitted.
        m.set_inline_budget(flower_core::InlineBudget::default());
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
    fn u_and_shift_u_walk_the_journal() {
        let mut m = model();
        m.focus_on(&[Seg::Key("version".into())]);
        press(&mut m, KeyCode::Char('e'));
        for c in "7".chars() {
            press(&mut m, KeyCode::Char(c));
        }
        press(&mut m, KeyCode::Enter);
        assert!(m.source_snapshot().contains("version = 17"));

        assert_eq!(press(&mut m, KeyCode::Char('u')), Outcome::Continue);
        assert_eq!(m.source_snapshot(), SAMPLE);
        assert!(!m.dirty, "back at the opened bytes");
        press(&mut m, KeyCode::Char('U'));
        assert!(m.source_snapshot().contains("version = 17"));

        // In edit mode they are text, like every other letter.
        press(&mut m, KeyCode::Char('e'));
        press(&mut m, KeyCode::Char('u'));
        assert!(matches!(m.mode, Mode::Editing { .. }));
        press(&mut m, KeyCode::Esc);
    }

    #[test]
    fn e_opens_the_picker_where_there_is_one_and_shift_e_types_anyway() {
        use flower_core::schema::{Constraint, Schema};
        use flower_core::{FieldRule, PathPat, Term};

        let backend = FigBackend::open(b"status = \"draft\"\n", fig::Format::Toml).expect("open");
        let mut m = Model::new(backend).expect("model");
        m.set_view(ViewMode::Pages);
        m.set_schema(Schema::new(vec![
            FieldRule::new(PathPat::key("status")).constraint(Constraint::Enum {
                values: vec![Term::value("draft"), Term::value("published")],
                closed: true,
            }),
        ]));
        m.focus_on(&[Seg::Key("status".into())]);

        assert_eq!(press(&mut m, KeyCode::Char('e')), Outcome::Continue);
        assert!(matches!(m.mode, Mode::Choosing { .. }));
        // Typing narrows, `j`/`k` walk, Enter commits.
        for c in "pub".chars() {
            press(&mut m, KeyCode::Char(c));
        }
        assert_eq!(m.visible_choices().len(), 1);
        press(&mut m, KeyCode::Enter);
        assert!(
            m.source_snapshot().contains("status = \"published\""),
            "{}",
            m.source_snapshot()
        );

        // `E` is the way to free text on the same field.
        press(&mut m, KeyCode::Char('E'));
        assert!(matches!(m.mode, Mode::Editing { .. }));
        press(&mut m, KeyCode::Esc);

        // Esc leaves the picker without writing.
        press(&mut m, KeyCode::Char('e'));
        press(&mut m, KeyCode::Esc);
        assert!(matches!(m.mode, Mode::Normal));
        assert!(m.source_snapshot().contains("status = \"published\""));
    }

    // ── the mouse ──────────────────────────────────────────────────────

    /// The 100×24 frame every mouse test draws into: a header row, a
    /// breadcrumb row, 21 rows of list, a footer.
    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 24,
    };

    fn mouse<B: Backend>(model: &mut Model<B>, kind: MouseEventKind, x: u16, y: u16) -> Outcome {
        handle_mouse(
            model,
            AREA,
            MouseEvent {
                kind,
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
        )
    }

    fn click<B: Backend>(model: &mut Model<B>, x: u16, y: u16) -> Outcome {
        mouse(model, MouseEventKind::Down(MouseButton::Left), x, y)
    }

    #[test]
    fn a_click_stands_on_the_row_and_a_second_click_opens_it() {
        let mut m = model();
        m.set_inline_budget(flower_core::InlineBudget::default());
        // Row 2 of the frame is item 0; `server` is item 2, on row 4.
        assert_eq!(click(&mut m, 5, 4), Outcome::Continue);
        assert_eq!(m.page_selected(), 2);
        assert!(m.focus().is_empty());
        click(&mut m, 5, 4);
        assert_eq!(m.focus(), [Seg::Key("server".into())]);
        // The cursor arrives on `host`; `port` is the row below. The second
        // click on a scalar opens it for editing, as Enter does.
        click(&mut m, 60, 3);
        assert_eq!(m.page_selected(), 1);
        assert!(matches!(m.mode, Mode::Normal));
        click(&mut m, 60, 3);
        assert!(matches!(m.mode, Mode::Editing { .. }));
    }

    #[test]
    fn a_click_on_the_chrome_or_past_the_editor_does_nothing() {
        let mut m = model();
        let was = m.page_selected();
        click(&mut m, 5, 0);
        click(&mut m, 5, 1);
        click(&mut m, 5, 23);
        click(&mut m, 120, 4);
        assert_eq!(m.page_selected(), was);
        assert!(m.focus().is_empty());
    }

    #[test]
    fn a_click_in_the_parent_pane_backs_out_onto_the_row_clicked() {
        let mut m = model();
        m.set_inline_budget(flower_core::InlineBudget::default());
        m.focus_on(&[Seg::Key("server".into())]);
        m.page_enter();
        assert!(!m.page_leads_the_split());
        // The left pane is the root page; `version` is its row 1, on frame row 3.
        click(&mut m, 5, 3);
        assert!(m.focus().is_empty());
        assert_eq!(m.page_selected(), 1);
    }

    #[test]
    fn a_click_in_the_preview_pane_opens_it_onto_the_row_clicked() {
        let mut m = model();
        m.set_inline_budget(flower_core::InlineBudget::default());
        m.focus_on(&[Seg::Key("server".into())]);
        assert!(m.page_leads_the_split());
        // The right pane previews `server`'s page; `port` is its row 1.
        click(&mut m, 60, 3);
        assert_eq!(m.focus(), [Seg::Key("server".into())]);
        assert_eq!(m.page_selected(), 1);
    }

    #[test]
    fn the_wheel_walks_the_cursor() {
        let mut m = model();
        let start = m.page_selected();
        mouse(&mut m, MouseEventKind::ScrollDown, 5, 4);
        mouse(&mut m, MouseEventKind::ScrollDown, 5, 4);
        assert_eq!(m.page_selected(), start + 2);
        // Over the other pane too: the wheel is not a pointer.
        mouse(&mut m, MouseEventKind::ScrollUp, 80, 4);
        assert_eq!(m.page_selected(), start + 1);
    }

    #[test]
    fn the_mouse_leaves_an_open_value_alone() {
        let mut m = model();
        press(&mut m, KeyCode::Char('e'));
        let was = m.page_selected();
        click(&mut m, 5, 4);
        mouse(&mut m, MouseEventKind::ScrollDown, 5, 4);
        assert!(matches!(m.mode, Mode::Editing { .. }));
        assert_eq!(m.page_selected(), was);
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
