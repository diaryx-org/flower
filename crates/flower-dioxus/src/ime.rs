//! The input method, for the edit line: dead keys, press-and-hold accents,
//! the emoji picker, dictation, and any input method that composes.
//!
//! winit reports the input method beside the keys: `Ime::Preedit` while a
//! composition is open, then `Ime::Commit` with what it settled on, and the
//! keystrokes a composition eats never arrive as `KeyboardInput`. The editor
//! allows the input method on the window only while a value is open — on a
//! page every letter is a command, and press-and-hold on `j` would open the
//! accent menu rather than walk the page.
//!
//! The composition is drawn after the buffer, underlined, and is not in it
//! until it commits, so the buffer is always the text outside the
//! composition — which is what the input method asks about. The edit line's
//! caret is always at the buffer's end, so the text around it is the buffer's
//! last line, and nothing comes after it.
//!
//! Press-and-hold needs that text, and on macOS only leaf's patched
//! winit-appkit answers the system with it (`apps/flower-dioxus`'s
//! `Cargo.toml`). Unpatched, the dead keys and composing input methods work,
//! and holding a key repeats it.

use flower_core::{Backend, Mode, Model};

/// The most text winit takes around the caret
/// (`ImeSurroundingText::MAX_TEXT_BYTES`).
const MAX: usize = 4000;

/// The text around the caret for the input method — the buffer's last line,
/// cut to winit's limit from its end — and the caret's byte offset in it,
/// which is its length. `None` with no value open: the input method is off.
pub fn surrounding<B: Backend>(model: &Model<B>) -> Option<(String, usize)> {
    let Mode::Editing { buffer, .. } = &model.mode else {
        return None;
    };
    let line = buffer.rfind('\n').map_or(0, |i| i + 1);
    let mut start = line.max(buffer.len().saturating_sub(MAX));
    while !buffer.is_char_boundary(start) {
        start += 1;
    }
    let text = buffer[start..].to_owned();
    let caret = text.len();
    Some((text, caret))
}

/// The input method wants `bytes` of text before the caret gone before its
/// next step: press-and-hold taking out the `e` it offers accents for. Whole
/// characters, at least `bytes` of them. Answers whether anything went.
pub fn delete_before<B: Backend>(model: &mut Model<B>, bytes: usize) -> bool {
    let Mode::Editing { buffer, .. } = &model.mode else {
        return false;
    };
    let mut gone = 0;
    let mut chars = 0;
    for c in buffer.chars().rev() {
        if gone >= bytes {
            break;
        }
        gone += c.len_utf8();
        chars += 1;
    }
    for _ in 0..chars {
        model.edit_backspace();
    }
    chars > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use flower_core::{FigBackend, Seg, ViewMode};

    use crate::input::insert_text;

    fn editing(seed: &str) -> Model<FigBackend> {
        let src = format!("title = \"{seed}\"\n");
        let b = FigBackend::open(src.as_bytes(), fig_toml::format().unwrap()).unwrap();
        let mut m = Model::new(b).unwrap();
        m.set_view(ViewMode::Pages);
        m.focus_on(&[Seg::Key("title".into())]);
        m.begin_edit();
        m
    }

    #[test]
    fn the_input_method_is_off_with_no_value_open() {
        let mut m = editing("x");
        m.edit_cancel();
        assert_eq!(surrounding(&m), None);
        assert!(!delete_before(&mut m, 1));
    }

    #[test]
    fn the_text_around_the_caret_is_the_buffers_last_line() {
        let mut m = editing("caf");
        assert_eq!(surrounding(&m), Some(("caf".into(), 3)));
        insert_text(&mut m, "é");
        m.edit_push('\n');
        m.edit_push('b');
        assert_eq!(surrounding(&m), Some(("b".into(), 1)));
    }

    #[test]
    fn a_long_line_keeps_its_end_and_never_splits_a_character() {
        let long = "é".repeat(MAX);
        let mut m = editing("");
        insert_text(&mut m, &long);
        let (text, caret) = surrounding(&m).unwrap();
        assert!(text.len() <= MAX);
        assert_eq!(caret, text.len());
        assert!(text.chars().all(|c| c == 'é'));
    }

    /// Press-and-hold on `e`: the `e` typed, then taken out for the accent
    /// chosen to replace it.
    #[test]
    fn press_and_hold_replaces_the_letter_it_offers_accents_for() {
        let mut m = editing("caf");
        m.edit_push('e');
        assert!(delete_before(&mut m, 1));
        insert_text(&mut m, "é");
        assert_eq!(surrounding(&m), Some(("café".into(), 5)));
        // Two bytes is one character here, not two.
        assert!(delete_before(&mut m, 2));
        assert_eq!(surrounding(&m), Some(("caf".into(), 3)));
    }
}
