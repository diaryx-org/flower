//! A glyph's toolkit-agnostic [`Style`] as CSS — the same job
//! `leaf-ratatui`'s `style.rs` does for a terminal cell, and that
//! `leaf-web`'s `editor.js` does for a `<span>`.
//!
//! Two halves, for the reason the measurements in README.md turn on: a
//! **class** for everything a stylesheet can say once (the roles, the syntax
//! tokens, the emphasis flags), and an **inline style** only for what varies
//! per run. A class is a short, shared string, so a run that differs from its
//! neighbour costs one `set_attribute` of a few bytes rather than a rebuilt
//! declaration list — and runs with the same role compare equal, which is what
//! lets a row memoize.

use flower_doc::{Baseline, MarkColor, Role, Style, Token};

/// The class list for a run of glyphs sharing one [`Style`].
///
/// Written into a reused `String` rather than returned, because the renderer
/// builds one per run per row and the allocation is the cost being measured.
pub fn class_of(style: &Style, out: &mut String) {
    out.push('g');
    match style.role {
        Role::Body => {}
        Role::Heading(level) => {
            out.push_str(" h");
            out.push((b'0' + level.clamp(1, 6)) as char);
        }
        Role::Code => out.push_str(" code"),
        Role::Link => out.push_str(" link"),
        Role::Mark(color) => {
            out.push_str(" mark");
            if let Some(c) = color {
                out.push(' ');
                out.push_str(mark_color(c));
            }
        }
        Role::ListMarker => out.push_str(" marker"),
        Role::ListIndent => out.push_str(" indent"),
        Role::QuoteGutter => out.push_str(" gutter"),
        Role::Rule => out.push_str(" rule"),
        Role::Delimiter => out.push_str(" delim"),
        Role::Image => out.push_str(" image"),
        Role::Math => out.push_str(" math"),
    }
    if style.bold {
        out.push_str(" b");
    }
    if style.italic {
        out.push_str(" i");
    }
    if style.underline {
        out.push_str(" u");
    }
    if style.strikethrough {
        out.push_str(" s");
    }
    match style.baseline {
        Baseline::Normal => {}
        Baseline::Super => out.push_str(" sup"),
        Baseline::Sub => out.push_str(" sub"),
    }
    // Only meaningful beside `Role::Code`, and `None` everywhere else, so a
    // document without a highlighted fence never pays for the branch.
    if let Some(token) = style.token {
        out.push(' ');
        out.push_str(token_class(token));
    }
}

/// The seven names a [`MarkColor`] takes, as class suffixes. The same seven a
/// [`flower_doc::TextColor`] carries, so a stylesheet spells each red once.
fn mark_color(c: MarkColor) -> &'static str {
    match c {
        MarkColor::Red => "red",
        MarkColor::Orange => "orange",
        MarkColor::Yellow => "yellow",
        MarkColor::Green => "green",
        MarkColor::Blue => "blue",
        MarkColor::Purple => "purple",
        MarkColor::Brown => "brown",
    }
}

/// syntect's eight-way class per code glyph. flower-doc picks the class; the
/// colour is this frontend's, as it is every frontend's.
fn token_class(t: Token) -> &'static str {
    match t {
        Token::Punctuation => "t-punct",
        Token::Keyword => "t-kw",
        Token::Entity => "t-entity",
        Token::Support => "t-support",
        Token::Constant => "t-const",
        Token::String => "t-str",
        Token::Comment => "t-comment",
        Token::Invalid => "t-invalid",
    }
}

/// The stylesheet the classes above are written against. A spike's worth of it
/// — enough that the rendered document is legible in a browser and that the
/// class strings are real, not enough to be a theme.
///
/// Every colour is a custom property, set once for a light appearance and
/// once for a dark one (`prefers-color-scheme`, which a browser, a webview
/// and Blitz all answer from the system's). The values are the Swift
/// editor's (`LeafUI/Platform.swift`'s `Palette`): AppKit's semantic colours
/// as each appearance resolves them, its syntax pairs, and its highlighter
/// washes — the system colours at 28%.
pub const STYLESHEET: &str = r#"
:root {
  color-scheme: light dark;
  --flower-bg: #ffffff;
  --flower-text: #262626;
  --flower-secondary: #808080;
  --flower-tertiary: #b3b3b3;
  --flower-accent: #007aff;
  --flower-misspelled: #ff3b30;
  --flower-link: #0068da;
  --flower-selection: #b3d7ff;
  --flower-code-bg: rgba(0, 0, 0, 0.05);
  --flower-separator: rgba(0, 0, 0, 0.12);
  --flower-table-head: rgba(0, 0, 0, 0.05);
  --flower-mark: rgba(255, 204, 0, 0.28);
  --flower-mark-red: rgba(255, 59, 48, 0.28);
  --flower-mark-orange: rgba(255, 149, 0, 0.28);
  --flower-mark-green: rgba(52, 199, 89, 0.28);
  --flower-mark-blue: rgba(0, 122, 255, 0.28);
  --flower-mark-purple: rgba(175, 82, 222, 0.28);
  --flower-mark-brown: rgba(162, 132, 94, 0.28);
  --flower-t-punct: #6a7580;
  --flower-t-kw: #9526a0;
  --flower-t-entity: #2b62d9;
  --flower-t-support: #016a99;
  --flower-t-const: #98590a;
  --flower-t-str: #0a7040;
  --flower-t-comment: #6a7580;
  --flower-t-invalid: #c02617;
}
@media (prefers-color-scheme: dark) {
  :root {
    --flower-bg: #1e1e1e;
    --flower-text: #dfdfdf;
    --flower-secondary: #8c8c8c;
    --flower-tertiary: #595959;
    --flower-accent: #0a84ff;
    --flower-misspelled: #ff453a;
    --flower-link: #419cff;
    --flower-selection: #3f638b;
    --flower-code-bg: rgba(255, 255, 255, 0.07);
    --flower-separator: rgba(255, 255, 255, 0.16);
    --flower-table-head: rgba(255, 255, 255, 0.07);
    --flower-mark: rgba(255, 214, 10, 0.28);
    --flower-mark-red: rgba(255, 69, 58, 0.28);
    --flower-mark-orange: rgba(255, 159, 10, 0.28);
    --flower-mark-green: rgba(48, 209, 88, 0.28);
    --flower-mark-blue: rgba(10, 132, 255, 0.28);
    --flower-mark-purple: rgba(191, 90, 242, 0.28);
    --flower-mark-brown: rgba(172, 142, 104, 0.28);
    --flower-t-punct: #7f8b99;
    --flower-t-kw: #c678dd;
    --flower-t-entity: #7aa2f7;
    --flower-t-support: #56b6c2;
    --flower-t-const: #e0af68;
    --flower-t-str: #9ece6a;
    --flower-t-comment: #7f8b99;
    --flower-t-invalid: #f7768e;
  }
}
.flower { font: 15px/1.5 ui-monospace, monospace; white-space: pre; tab-size: 4;
        caret-color: transparent; outline: none; color: var(--flower-text); }
.flower .row { position: relative; min-height: 1.5em; }
.flower .row.code { background: var(--flower-code-bg); }
.flower .row.decoration { color: var(--flower-secondary); }
.flower .caret { position: absolute; width: 2px; background: var(--flower-accent);
               top: 0.1em; bottom: 0.1em; animation: flower-blink 1s step-end infinite; }
@keyframes flower-blink { 50% { opacity: 0; } }
.flower .sel { background: var(--flower-selection); }
.flower .h1 { font-size: 2em; font-weight: 700; }
.flower .h2 { font-size: 1.6em; font-weight: 700; }
.flower .h3 { font-size: 1.3em; font-weight: 700; }
.flower .h4, .flower .h5, .flower .h6 { font-weight: 700; }
.flower .b { font-weight: 700; }
.flower .i { font-style: italic; }
.flower .u { text-decoration: underline; }
.flower .s { text-decoration: line-through; }
.flower .sup { vertical-align: super; font-size: 0.8em; }
.flower .sub { vertical-align: sub; font-size: 0.8em; }
.flower .link { color: var(--flower-link); text-decoration: underline; }
.flower .delim { color: var(--flower-secondary); }
.flower .marker { color: var(--flower-secondary); }
.flower .gutter { color: var(--flower-tertiary); }
.flower .rule { color: var(--flower-secondary); }
.flower .mark, .flower .hl { background: var(--flower-mark); }
.flower .hl.red { background: var(--flower-mark-red); }
.flower .hl.orange { background: var(--flower-mark-orange); }
.flower .hl.green { background: var(--flower-mark-green); }
.flower .hl.blue { background: var(--flower-mark-blue); }
.flower .hl.purple { background: var(--flower-mark-purple); }
.flower .hl.brown { background: var(--flower-mark-brown); }
.flower .mark.red { background: var(--flower-mark-red); }
.flower .mark.orange { background: var(--flower-mark-orange); }
.flower .mark.yellow { background: var(--flower-mark); }
.flower .mark.green { background: var(--flower-mark-green); }
.flower .mark.blue { background: var(--flower-mark-blue); }
.flower .mark.purple { background: var(--flower-mark-purple); }
.flower .mark.brown { background: var(--flower-mark-brown); }
.flower .t-punct { color: var(--flower-t-punct); }
.flower .t-kw { color: var(--flower-t-kw); }
.flower .t-entity { color: var(--flower-t-entity); }
.flower .t-support { color: var(--flower-t-support); }
.flower .t-const { color: var(--flower-t-const); }
.flower .t-str { color: var(--flower-t-str); }
.flower .t-comment { color: var(--flower-t-comment); font-style: italic; }
.flower .t-invalid { color: var(--flower-t-invalid); text-decoration: underline wavy; }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_text_is_one_class() {
        let mut s = String::new();
        class_of(&Style::default(), &mut s);
        assert_eq!(s, "g");
    }

    #[test]
    fn a_bold_heading_names_its_level() {
        let mut s = String::new();
        class_of(&Style::default().role(Role::Heading(2)).bold(), &mut s);
        assert_eq!(s, "g h2 b");
    }

    #[test]
    fn a_code_glyph_carries_its_token() {
        let mut style = Style::default().role(Role::Code);
        style.token = Some(Token::Keyword);
        let mut s = String::new();
        class_of(&style, &mut s);
        assert_eq!(s, "g code t-kw");
    }
}
