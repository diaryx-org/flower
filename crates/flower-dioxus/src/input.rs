//! The key table: what a key does to a [`Model`], in the three modes it can be
//! in, and which keys are the host's to answer ([`Request`]).
//!
//! The same table as `flower-ratatui`'s, so a reader moves between the
//! terminal and the window without relearning it — `j`/`k` and the arrows walk
//! a page, `l`/`h` open and leave one, `e` picks or edits a value, `a` adds to
//! a list, `c`/`C` edit the comments, `x` deletes, `u`/`U` undo and redo —
//! with what a window adds: ⌘Z and ⇧⌘Z, ⌫ and ⌦ to delete, ⌥↑ and ⌥↓ to move
//! an entry, and ⌘S, ⇧⌘S and the zoom for the host. Quitting is not here: a
//! window is closed by its host, the way every other window is.
//!
//! A chord with ⌘ or ⌃ that is not one of these is not the editor's, so a
//! host's menu shortcut never types a letter into a value or deletes a row.

use dioxus::html::{Key, Modifiers};
use flower_core::{Backend, Mode, Model};

use crate::host::Request;

/// What a key came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Not the editor's: the host may do what it likes with it.
    Ignored,
    /// Done to the model; repaint.
    Handled,
    /// One of the editor's keys, for the host to carry out.
    Request(Request),
    /// ⌘V while a value is open: the host's clipboard, into the buffer.
    Paste,
}

/// ⌘+ (or ⌘=), ⌘−, ⌘0: the type set larger, smaller, or back to its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomCommand {
    In,
    Out,
    Reset,
}

/// Apply `key` to `model`.
pub fn apply_key<B: Backend>(model: &mut Model<B>, key: &Key, mods: Modifiers) -> Action {
    let command = mods.contains(Modifiers::SUPER);
    let control = mods.contains(Modifiers::CONTROL);
    let shift = mods.contains(Modifiers::SHIFT);
    let alt = mods.contains(Modifiers::ALT);

    // The host's, in every mode: a save with a value half typed saves what
    // the document says, and the open value stays open.
    if command && let Key::Character(c) = key {
        match c.to_lowercase().as_str() {
            "s" if shift => return Action::Request(Request::SaveAs),
            "s" => return Action::Request(Request::Save),
            "=" | "+" => return Action::Request(Request::Zoom(ZoomCommand::In)),
            "-" => return Action::Request(Request::Zoom(ZoomCommand::Out)),
            "0" => return Action::Request(Request::Zoom(ZoomCommand::Reset)),
            _ => {}
        }
    }

    match model.mode {
        Mode::Editing { .. } => editing(model, key, command, control),
        Mode::Choosing { .. } => choosing(model, key, command, control),
        Mode::Normal => normal(model, key, command, control, shift, alt),
    }
}

fn normal<B: Backend>(
    model: &mut Model<B>,
    key: &Key,
    command: bool,
    control: bool,
    shift: bool,
    alt: bool,
) -> Action {
    match key {
        Key::Character(c) if command => match c.to_lowercase().as_str() {
            "z" if shift => {
                model.redo();
            }
            "z" => {
                model.undo();
            }
            _ => return Action::Ignored,
        },
        // ⌘⌫, as the Finder deletes.
        Key::Backspace if command => model.delete_selected(),
        _ if command || control => return Action::Ignored,
        Key::Character(c) => match c.as_str() {
            // The picker where the field has one, the text line where it does
            // not; `E` is the way past the picker.
            "e" => model.begin_choose(),
            "E" => model.begin_edit(),
            "a" | "A" => match model.append_target() {
                Some(seq) if c == "a" => model.begin_choose_append(&seq),
                Some(seq) => model.begin_append(&seq),
                None => model.reject(None, "nothing here takes a new item — add to a list"),
            },
            "c" => model.begin_edit_trailing_comment(),
            "C" => model.begin_edit_leading_comment(),
            "x" => model.delete_selected(),
            "u" => {
                model.undo();
            }
            "U" => {
                model.redo();
            }
            "j" => model.page_move_down(),
            "k" => model.page_move_up(),
            "l" | " " => model.page_enter(),
            "h" => model.page_back(),
            _ => return Action::Ignored,
        },
        Key::ArrowDown if alt => model.move_selected_down(),
        Key::ArrowUp if alt => model.move_selected_up(),
        Key::ArrowDown => model.page_move_down(),
        Key::ArrowUp => model.page_move_up(),
        Key::ArrowRight | Key::Enter => model.page_enter(),
        Key::ArrowLeft | Key::Escape => model.page_back(),
        Key::Delete => model.delete_selected(),
        _ => return Action::Ignored,
    }
    Action::Handled
}

/// Typing into the edit line.
fn editing<B: Backend>(model: &mut Model<B>, key: &Key, command: bool, control: bool) -> Action {
    match key {
        Key::Character(c) if command && c.eq_ignore_ascii_case("v") => return Action::Paste,
        _ if command || control => return Action::Ignored,
        Key::Character(c) => {
            for ch in c.chars() {
                model.edit_push(ch);
            }
        }
        Key::Backspace => model.edit_backspace(),
        Key::Enter => model.edit_commit(),
        Key::Escape => model.edit_cancel(),
        _ => return Action::Ignored,
    }
    Action::Handled
}

/// Driving the picker: `j`/`k` and the arrows walk it, as on the page, and
/// everything else printable narrows it.
fn choosing<B: Backend>(model: &mut Model<B>, key: &Key, command: bool, control: bool) -> Action {
    match key {
        _ if command || control => return Action::Ignored,
        Key::Character(c) if c == "j" => model.choose_next(),
        Key::Character(c) if c == "k" => model.choose_prev(),
        Key::Character(c) => {
            for ch in c.chars() {
                model.choose_push(ch);
            }
        }
        Key::ArrowDown => model.choose_next(),
        Key::ArrowUp => model.choose_prev(),
        Key::Backspace => model.choose_backspace(),
        Key::Enter => model.choose_commit(),
        Key::Escape => model.choose_cancel(),
        _ => return Action::Ignored,
    }
    Action::Handled
}

/// Text from outside — a paste, an input method's commit — into whatever is
/// open: the edit line's buffer, or the picker's filter. One line of it: the
/// line is one line. Answers whether anything took it.
pub fn insert_text<B: Backend>(model: &mut Model<B>, text: &str) -> bool {
    let line = text.lines().next().unwrap_or_default();
    match model.mode {
        Mode::Editing { .. } => line.chars().for_each(|c| model.edit_push(c)),
        Mode::Choosing { .. } => line.chars().for_each(|c| model.choose_push(c)),
        Mode::Normal => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use flower_core::{FigBackend, Seg, ViewMode};

    const SAMPLE: &str = "title = \"flower\"\nversion = 1\ntags = [\"a\"]\n";

    fn model() -> Model<FigBackend> {
        let b = FigBackend::open(SAMPLE.as_bytes(), fig_toml::format().unwrap()).unwrap();
        let mut m = Model::new(b).unwrap();
        m.set_view(ViewMode::Pages);
        m
    }

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    fn press(m: &mut Model<FigBackend>, key: Key) -> Action {
        apply_key(m, &key, Modifiers::empty())
    }

    #[test]
    fn the_terminals_keys_walk_and_edit_the_page() {
        let mut m = model();
        let start = m.page_selected();
        press(&mut m, ch("j"));
        assert_eq!(m.page_selected(), start + 1);
        press(&mut m, Key::ArrowUp);
        assert_eq!(m.page_selected(), start);

        m.focus_on(&[Seg::Key("version".into())]);
        press(&mut m, ch("e"));
        assert!(matches!(m.mode, Mode::Editing { .. }));
        // Letters are text while a value is open, the commands' included.
        for c in ["7", "x"] {
            press(&mut m, ch(c));
        }
        press(&mut m, Key::Backspace);
        press(&mut m, Key::Enter);
        assert!(m.source_snapshot().contains("version = 17"));
        assert_eq!(
            apply_key(&mut m, &ch("z"), Modifiers::SUPER),
            Action::Handled
        );
        assert_eq!(m.source_snapshot(), SAMPLE);
        apply_key(&mut m, &ch("Z"), Modifiers::SUPER | Modifiers::SHIFT);
        assert!(m.source_snapshot().contains("version = 17"));
    }

    #[test]
    fn save_and_the_zoom_are_the_hosts_and_nothing_else_with_command_is_ours() {
        let mut m = model();
        assert_eq!(
            apply_key(&mut m, &ch("s"), Modifiers::SUPER),
            Action::Request(Request::Save)
        );
        assert_eq!(
            apply_key(&mut m, &ch("S"), Modifiers::SUPER | Modifiers::SHIFT),
            Action::Request(Request::SaveAs)
        );
        assert_eq!(
            apply_key(&mut m, &ch("="), Modifiers::SUPER),
            Action::Request(Request::Zoom(ZoomCommand::In))
        );
        // ⌘X is a menu's, not `x`'s: nothing is deleted.
        assert_eq!(
            apply_key(&mut m, &ch("x"), Modifiers::SUPER),
            Action::Ignored
        );
        assert!(!m.dirty);
    }

    #[test]
    fn the_delete_keys_delete_and_escape_cancels_an_open_value() {
        let mut m = model();
        m.focus_on(&[Seg::Key("title".into())]);
        press(&mut m, ch("e"));
        press(&mut m, ch("q"));
        press(&mut m, Key::Escape);
        assert!(matches!(m.mode, Mode::Normal));
        assert!(!m.dirty);
        press(&mut m, Key::Delete);
        assert!(!m.source_snapshot().contains("title"));
        press(&mut m, ch("u"));
        apply_key(&mut m, &Key::Backspace, Modifiers::SUPER);
        assert!(!m.source_snapshot().contains("title"));
    }

    #[test]
    fn a_paste_and_an_input_method_type_into_what_is_open() {
        let mut m = model();
        m.focus_on(&[Seg::Key("title".into())]);
        assert!(!insert_text(&mut m, "ignored"));
        press(&mut m, ch("E"));
        assert_eq!(apply_key(&mut m, &ch("v"), Modifiers::SUPER), Action::Paste);
        assert!(insert_text(&mut m, "é\nsecond line"));
        press(&mut m, Key::Enter);
        assert!(
            m.source_snapshot().contains("title = \"floweré\""),
            "{}",
            m.source_snapshot()
        );
    }

    #[test]
    fn a_adds_to_the_list_the_cursor_is_on() {
        let mut m = model();
        m.focus_on(&[Seg::Key("tags".into())]);
        press(&mut m, ch("A"));
        press(&mut m, ch("b"));
        press(&mut m, Key::Enter);
        assert!(
            m.source_snapshot().contains("tags = [\"a\", \"b\"]"),
            "{}",
            m.source_snapshot()
        );
        m.focus_on(&[Seg::Key("title".into())]);
        press(&mut m, ch("a"));
        assert!(matches!(m.mode, Mode::Normal));
        assert!(m.status.starts_with("nothing here takes a new item"));
    }
}
