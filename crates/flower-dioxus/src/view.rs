//! The page: the stylesheet, light and dark, and the markup — one or two
//! panes of entries, the picker over them, and the footer that doubles as the
//! edit line.

use std::collections::BTreeSet;

use dioxus::html::geometry::WheelDelta;
use dioxus::prelude::*;
use flower_core::{Backend, ChoiceTarget, EditSlot, Mode, Model, NoticeKind};

use crate::editor::{Editor, FOOTER, Geometry, Look, PAD, TOP};
use crate::layout::{self, Entry, Role, Shown, fit};

/// Light or dark: the system's, or one of the two whatever the system says.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

/// How the document root reads in a breadcrumb.
const ROOT_LABEL: &str = "Document";

/// The footer's type size in CSS pixels — `.footer` in the stylesheet.
pub const FOOTER_TYPE: f32 = 12.0;

/// The picker's most rows at once.
const CHOICES_SHOWN: usize = 12;

/// The colours, once per appearance. Every colour the stylesheet uses is a
/// custom property set in both blocks — the test below checks it — so a
/// forced appearance is one switch ([`themed`]).
const PALETTE: &str = "
:root {
  --fl-bg: #ffffff;
  --fl-text: #1f1f1f;
  --fl-dim: #8a8a8a;
  --fl-key: #1f4fbf;
  --fl-null: #8a8a8a;
  --fl-bool: #9a2fa3;
  --fl-num: #0b7a86;
  --fl-str: #2d7a2d;
  --fl-ext: #9a6a00;
  --fl-error: #c62828;
  --fl-warning: #b07800;
  --fl-sel: #e4ecfb;
  --fl-sel-quiet: #f0f0f0;
  --fl-sel-strong: #1f4fbf;
  --fl-sel-strong-text: #ffffff;
  --fl-chrome: #f5f5f5;
  --fl-separator: #dedede;
  --fl-accent: #1f6feb;
  --fl-badge-text: #ffffff;
  --fl-done: #2d7a2d;
  --fl-overlay: #ffffff;
}
@media (prefers-color-scheme: dark) {
  :root {
    --fl-bg: #1e1e1e;
    --fl-text: #e2e2e2;
    --fl-dim: #8c8c8c;
    --fl-key: #7aa7ff;
    --fl-null: #8c8c8c;
    --fl-bool: #d58ae0;
    --fl-num: #5fc4cf;
    --fl-str: #8fd18f;
    --fl-ext: #e0b45a;
    --fl-error: #ff6b6b;
    --fl-warning: #e0b45a;
    --fl-sel: #2c3550;
    --fl-sel-quiet: #2b2b2b;
    --fl-sel-strong: #7aa7ff;
    --fl-sel-strong-text: #10172a;
    --fl-chrome: #2a2a2a;
    --fl-separator: #3d3d3d;
    --fl-accent: #4c8dff;
    --fl-badge-text: #10172a;
    --fl-done: #8fd18f;
    --fl-overlay: #262626;
  }
}
";

/// The editor's stylesheet for `look`.
///
/// Two Blitz behaviours shape it, both found by leaf-dioxus: it paints the
/// first frame of an `overflow: hidden` element and not its changes, so the
/// clip is on `body` and never on a pane; and it lays out an absolutely
/// positioned element on the first paint and not when a later frame inserts
/// it, so the picker and the footer are always in the page, the picker
/// hidden while it is closed.
pub fn css(look: &Look) -> String {
    let Look { mono, size, .. } = look;
    let line = look.line();
    format!(
        "{PALETTE}
html, body {{ margin: 0; overflow: hidden; height: 100vh; background: var(--fl-bg); }}
body {{ position: relative; }}
.flower {{ font-family: {mono}; font-size: {size}px; line-height: {line}px; color: var(--fl-text); }}
.flower .panes {{ display: flex; padding-top: {TOP}px; }}
.flower .pane {{ flex: none; box-sizing: border-box; padding: 0 {PAD}px; }}
.flower .pane + .pane {{ border-left: 1px solid var(--fl-separator); }}
.flower .crumb {{ height: {line}px; white-space: pre; font-weight: bold; }}
.flower .crumb.other {{ color: var(--fl-dim); font-weight: normal; }}
.flower .entry {{ border-radius: 4px; }}
.flower .entry.sel {{ background: var(--fl-sel); }}
.flower .pane.other .entry.sel {{ background: var(--fl-sel-quiet); }}
.flower .line {{ height: {line}px; white-space: pre; }}
.flower .placeholder {{ color: var(--fl-dim); white-space: pre; }}
.flower .t {{ white-space: pre; }}
.flower .dim {{ color: var(--fl-dim); }}
.flower .key {{ color: var(--fl-key); font-weight: bold; }}
.flower .v-null {{ color: var(--fl-null); }}
.flower .v-bool {{ color: var(--fl-bool); }}
.flower .v-num {{ color: var(--fl-num); }}
.flower .v-str {{ color: var(--fl-str); }}
.flower .v-ext {{ color: var(--fl-ext); }}
.flower .mark-error {{ color: var(--fl-error); font-weight: bold; }}
.flower .mark-warning {{ color: var(--fl-warning); }}
.flower .chip-sel {{ background: var(--fl-sel-strong); color: var(--fl-sel-strong-text); border-radius: 3px; }}
.flower .picker {{ position: absolute; z-index: 2; top: {top}px; box-sizing: border-box;
                   background: var(--fl-overlay); border: 1px solid var(--fl-accent); border-radius: 6px; padding: 4px 0; }}
.flower .picker .title {{ padding: 0 10px; white-space: pre; color: var(--fl-accent); font-weight: bold; }}
.flower .picker .choice {{ padding: 0 10px; white-space: pre; }}
.flower .picker .choice.sel {{ background: var(--fl-sel); }}
.flower .footer {{ position: absolute; z-index: 1; left: 0; right: 0; bottom: 0; height: {FOOTER}px;
                   line-height: {FOOTER}px; padding: 0 {PAD}px; box-sizing: border-box; white-space: pre;
                   font-size: {FOOTER_TYPE}px; color: var(--fl-dim); background: var(--fl-chrome);
                   border-top: 1px solid var(--fl-separator); display: flex; gap: 10px; }}
.flower .footer .grow {{ flex: 1; }}
.flower .badge {{ padding: 0 6px; border-radius: 4px; height: 18px; line-height: 18px; margin-top: 4px;
                  color: var(--fl-badge-text); background: var(--fl-accent); }}
.flower .badge.done {{ background: var(--fl-done); }}
.flower .badge.rejected {{ background: var(--fl-error); }}
.flower .badge.warning {{ background: var(--fl-warning); }}
.flower .footer .buffer {{ color: var(--fl-text); }}
.flower .footer .marked {{ text-decoration: underline; text-decoration-color: var(--fl-accent); }}
.flower .caret {{ display: inline-block; width: 0; height: 1.2em; vertical-align: text-bottom;
                  border-left: 2px solid var(--fl-accent); margin-right: -2px; }}
",
        top = TOP + line,
    )
}

/// The stylesheet for an appearance. `System` leaves the dark palette to
/// `prefers-color-scheme`, which Blitz answers from winit's theme. A forced
/// one switches the dark block on or off itself: winit sends no
/// `ThemeChanged` for the app's own `Window::set_theme`, so Blitz never hears
/// of it.
pub fn themed(css: &str, appearance: Appearance) -> String {
    const DARK: &str = "@media (prefers-color-scheme: dark)";
    match appearance {
        Appearance::System => css.to_string(),
        Appearance::Dark => css.replace(DARK, "@media all"),
        Appearance::Light => css.replace(DARK, "@media not all"),
    }
}

/// The custom properties a stylesheet uses, and those it sets for each
/// appearance — for checking that every one is set for both, the editor's
/// and a host's own chrome drawn in them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Palette {
    pub used: BTreeSet<String>,
    pub light: BTreeSet<String>,
    pub dark: BTreeSet<String>,
}

impl Palette {
    /// Read off `css`, laid out as the editor's is: a light `:root { … }`,
    /// and a dark one first inside each `@media (prefers-color-scheme:
    /// dark)`.
    pub fn of(css: &str) -> Self {
        let used = css
            .split("var(--")
            .skip(1)
            .map(|t| {
                t.split(|c: char| !(c.is_alphanumeric() || c == '-'))
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .collect();
        let (mut light, mut dark) = (BTreeSet::new(), BTreeSet::new());
        for (k, part) in css.split("@media (prefers-color-scheme: dark)").enumerate() {
            for (n, block) in part.split(":root {").skip(1).enumerate() {
                let body = block.split('}').next().unwrap_or("");
                let defined = body
                    .lines()
                    .filter_map(|l| l.trim().strip_prefix("--"))
                    .filter_map(|l| l.split(':').next())
                    .map(str::to_string);
                if k > 0 && n == 0 {
                    &mut dark
                } else {
                    &mut light
                }
                .extend(defined);
            }
        }
        Self { used, light, dark }
    }

    /// That both appearances set the same properties, and every property
    /// used is set.
    pub fn check(&self) -> Result<(), String> {
        if self.light != self.dark {
            let only: Vec<_> = self.light.symmetric_difference(&self.dark).collect();
            return Err(format!("set for one appearance only: {only:?}"));
        }
        let missing: Vec<_> = self.used.difference(&self.light).collect();
        if !missing.is_empty() {
            return Err(format!("used but never set: {missing:?}"));
        }
        Ok(())
    }
}

impl<B: Backend + 'static> Editor<B> {
    /// The editor's part of the page: its stylesheet, the panes, the picker
    /// and the footer. A host draws its own beside it.
    pub fn render(&self) -> Element {
        render(self)
    }
}

fn render<B: Backend + 'static>(ed: &Editor<B>) -> Element {
    // Read, so a change to any of them draws the page again.
    let _ = *ed.sig.tick.read();
    let (width, height) = *ed.sig.size.read();
    let look = ed.sig.look.read().clone();
    let g = ed.geometry();
    let model = ed.st.model.borrow();
    let shown = layout::panes(&*model, g.two);
    let picking = matches!(model.mode, Mode::Choosing { .. });
    let wheel = ed.clone();
    let away = ed.clone();

    rsx! {
        style { {themed(&css(&look), look.appearance)} }
        div { class: "flower", style: "width: {width}px; height: {height}px",
            // The wheel walks the cursor; the mouse's default actions are
            // Blitz's own text selection and the focus changes that make
            // `onkeydown` unreliable, so they are cancelled.
            onwheel: move |e: WheelEvent| {
                e.prevent_default();
                wheel.wheel(match e.delta() {
                    WheelDelta::Pixels(v) => v.y,
                    WheelDelta::Lines(v) => v.y * 20.0,
                    WheelDelta::Pages(v) => v.y * height as f64,
                });
            },
            onmousedown: move |e: MouseEvent| {
                e.prevent_default();
                away.dismiss();
            },
            div { class: "panes",
                for (n, pane) in shown.iter().enumerate() {
                    match pane {
                        Some(s) => pane_view(ed, s, &g, n),
                        None => rsx! {
                            div { key: "{n}", class: "pane other", style: "width: {g.pane_width}px",
                                div { class: "crumb other", "" }
                                div { class: "placeholder", "  nothing to open here" }
                            }
                        },
                    }
                }
            }
            {picker(ed, &model, &g, width, picking)}
            {footer(&model, &ed.st.marked.borrow())}
        }
    }
}

/// One pane: its breadcrumb over the entries placed in it.
fn pane_view<B: Backend + 'static>(
    ed: &Editor<B>,
    s: &Shown<'_>,
    g: &Geometry,
    n: usize,
) -> Element {
    let current = s.role == Role::Current;
    let class = if current { "pane" } else { "pane other" };
    let crumb_class = if current { "crumb" } else { "crumb other" };
    let crumb = fit(&s.page.breadcrumb(ROOT_LABEL), g.cells);
    let entries = layout::entries(&s.page, g.cells);
    let col = layout::value_col(&entries, g.cells);
    let placed = layout::place(&entries, s.selected, g.room);
    rsx! {
        div { key: "{n}", class, style: "width: {g.pane_width}px",
            div { class: crumb_class, "{crumb}" }
            if s.page.is_empty() {
                div { class: "placeholder", "  (empty)" }
            }
            for p in placed {
                {entry_view(ed, s, &entries[p.entry], p.lines, col, g)}
            }
        }
    }
}

/// One entry: the lines it takes, of the lines it was given room for.
fn entry_view<B: Backend + 'static>(
    ed: &Editor<B>,
    s: &Shown<'_>,
    entry: &Entry<'_>,
    room: usize,
    col: usize,
    g: &Geometry,
) -> Element {
    let role = s.role;
    let index = entry.index();
    let sel = s.selected.filter(|&i| entry.holds(i));
    let class = if sel.is_some() { "entry sel" } else { "entry" };
    let lines = layout::lines_of(entry, col, g.cells);
    // A clipped entry keeps its last lines, the item's own among them.
    let skip = lines.len().saturating_sub(room);
    let press = ed.clone();
    rsx! {
        div { key: "{index}", class,
            onmousedown: move |e: MouseEvent| {
                e.prevent_default();
                e.stop_propagation();
                press.press(role, index);
            },
            for (k, line) in lines.into_iter().enumerate().skip(skip) {
                div { key: "{k}", class: "line",
                    for (r, run) in line.into_iter().enumerate() {
                        match run.chip {
                            Some(item) => {
                                let class = if sel == Some(item) {
                                    format!("{} chip-sel", run.ink.class())
                                } else {
                                    run.ink.class().to_string()
                                };
                                let press = ed.clone();
                                rsx! {
                                    span { key: "{r}", class,
                                        onmousedown: move |e: MouseEvent| {
                                            e.prevent_default();
                                            e.stop_propagation();
                                            press.press(role, item);
                                        },
                                        "{run.text}"
                                    }
                                }
                            }
                            None => rsx! { span { key: "{r}", class: run.ink.class(), "{run.text}" } },
                        }
                    }
                }
            }
        }
    }
}

/// The picker: a box over the page, the filter in its title, the choices
/// scrolled only as far as the cursor needs. Always in the page, hidden
/// while closed (see [`css`]).
fn picker<B: Backend + 'static>(
    ed: &Editor<B>,
    model: &Model<B>,
    g: &Geometry,
    width: f32,
    open: bool,
) -> Element {
    let display = if open { "block" } else { "none" };
    let cells = g.cells.clamp(20, 56);
    let w = cells as f32 * ed.look().cell() + 20.0;
    let left = ((width - w) / 2.0).max(0.0);
    let (selected, filter, adding) = match &model.mode {
        Mode::Choosing {
            selected,
            filter,
            target,
            ..
        } => (
            *selected,
            filter.as_str(),
            matches!(target, ChoiceTarget::Append(_)),
        ),
        _ => (0, "", false),
    };
    let verb = if adding { "add" } else { "choose" };
    let title = if filter.is_empty() {
        verb.to_string()
    } else {
        format!("{verb}: {filter}")
    };
    let choices = if open {
        model.visible_choices()
    } else {
        Vec::new()
    };
    let first = selected.saturating_sub(CHOICES_SHOWN - 1);
    let rows: Vec<(usize, String, bool)> = choices
        .iter()
        .enumerate()
        .skip(first)
        .take(CHOICES_SHOWN)
        .map(|(i, c)| {
            let mut text = c.label.clone();
            if let Some(detail) = &c.detail {
                text = format!("{text}  {detail}");
            }
            (i, fit(&text, cells), i == selected)
        })
        .collect();
    let empty = open && choices.is_empty();
    rsx! {
        div { class: "picker", style: "display: {display}; left: {left}px; width: {w}px",
            onmousedown: move |e: MouseEvent| {
                e.prevent_default();
                e.stop_propagation();
            },
            div { class: "title", "{title}" }
            if empty {
                div { class: "choice dim", "nothing matches" }
            }
            for (i, text, sel) in rows {
                div { key: "{i}", class: if sel { "choice sel" } else { "choice" },
                    onmousedown: {
                        let ed = ed.clone();
                        move |e: MouseEvent| {
                            e.prevent_default();
                            e.stop_propagation();
                            ed.choose(i);
                        }
                    },
                    "{text}"
                }
            }
        }
    }
}

/// The edit line's badge: what the open value is.
pub(crate) fn edit_badge(slot: &EditSlot) -> &'static str {
    match slot {
        EditSlot::Value => "edit",
        EditSlot::TrailingComment => "comment",
        EditSlot::LeadingComment => "comment above",
        EditSlot::NewItem => "add",
    }
}

/// The footer: the edit line while a value is open, what the picker takes
/// while it is, and otherwise the last thing the model had to say — or the
/// host's finding on the row the cursor is on — and the keys. `marked` is the
/// input method's composition, drawn after the buffer.
fn footer<B: Backend>(model: &Model<B>, marked: &str) -> Element {
    const HINTS: &str =
        "↑↓ move · → open · ← back · e edit · a add · c/C comment · ⌫ delete · ⌘Z undo · ⌘S save";
    match &model.mode {
        Mode::Editing { buffer, slot, .. } => {
            let badge = edit_badge(slot);
            // The line is one line: a leading block's breaks are shown as a
            // mark rather than lost.
            let shown = buffer.replace('\n', "⏎");
            rsx! {
                div { class: "footer",
                    span { class: "badge", "{badge}" }
                    span { class: "buffer", "{shown}",
                        span { class: "marked", "{marked}" }
                        span { class: "caret" }
                    }
                    span { class: "grow" }
                    span { "Return to commit · Esc to cancel" }
                }
            }
        }
        Mode::Choosing { target, .. } => {
            let verb = match target {
                ChoiceTarget::Append(_) => "add",
                ChoiceTarget::Replace(_) => "choose",
            };
            rsx! {
                div { class: "footer",
                    span { class: "badge", "{verb}" }
                    span { "type to filter · ↑↓ · Return to choose · Esc to cancel" }
                }
            }
        }
        Mode::Normal => {
            let said = model
                .notice
                .as_ref()
                .filter(|_| !model.status.is_empty())
                .map(|n| {
                    let class = match n.kind {
                        NoticeKind::Done => "badge done",
                        NoticeKind::Warning => "badge warning",
                        NoticeKind::Rejected => "badge rejected",
                    };
                    (class, n.message.clone())
                })
                .or_else(|| {
                    (!model.status.is_empty()).then(|| ("badge done", model.status.clone()))
                })
                .or_else(|| {
                    model
                        .page_item()
                        .and_then(|i| i.annotation.as_ref())
                        .map(|a| {
                            let class = match a.severity {
                                flower_core::annotate::Severity::Error => "badge rejected",
                                flower_core::annotate::Severity::Warning => "badge warning",
                                flower_core::annotate::Severity::Info => "badge",
                            };
                            (class, a.message.clone())
                        })
                });
            rsx! {
                div { class: "footer",
                    if let Some((class, text)) = said {
                        span { class, "{text}" }
                    }
                    span { class: "grow" }
                    span { "{HINTS}" }
                }
            }
        }
    }
}

/// The inks [`layout`] draws runs in, each a class [`css`] sets.
#[cfg(test)]
fn every_ink() -> Vec<crate::layout::Ink> {
    use crate::layout::Ink;
    use flower_core::VKind;
    use flower_core::annotate::Severity;
    let mut v = vec![Ink::Plain, Ink::Dim, Ink::Key];
    for k in [
        VKind::Null,
        VKind::Bool,
        VKind::Int,
        VKind::Float,
        VKind::Str,
        VKind::Ext,
        VKind::Map,
        VKind::Seq,
    ] {
        v.push(Ink::Value(k));
    }
    for s in [Severity::Error, Severity::Warning, Severity::Info] {
        v.push(Ink::Marker(s));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_colour_is_set_for_both_appearances_and_none_is_spelled_outside_them() {
        let all = css(&Look::default());
        let palette = Palette::of(&all);
        assert!(palette.used.len() > 15, "{:?}", palette.used);
        palette.check().expect("a whole palette");
        let rules = all.split("--fl-overlay: #262626;").last().unwrap();
        assert!(
            !rules.contains('#'),
            "a literal colour outside the palettes"
        );
    }

    #[test]
    fn every_ink_has_a_rule() {
        let all = css(&Look::default());
        for ink in every_ink() {
            for class in ink.class().split(' ') {
                assert!(all.contains(&format!(".{class} ")), "no rule for .{class}");
            }
        }
    }

    #[test]
    fn a_forced_appearance_switches_the_dark_block_itself() {
        let all = css(&Look::default());
        assert_eq!(themed(&all, Appearance::System), all);
        assert!(themed(&all, Appearance::Dark).contains("@media all"));
        assert!(themed(&all, Appearance::Light).contains("@media not all"));
    }

    #[test]
    fn the_stylesheet_sets_the_type_the_cells_are_counted_in() {
        let look = Look {
            mono: "Monaco, monospace".into(),
            size: 18.0,
            ..Look::default()
        };
        let all = css(&look);
        assert!(
            all.contains("font-family: Monaco, monospace; font-size: 18px;"),
            "{all}"
        );
        assert!(all.contains(&format!("line-height: {}px", look.line())));
    }
}
