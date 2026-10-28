//! The page: the editor's stylesheet and its markup — the breadcrumb's bar,
//! one or two panes of sections, and the picker — out of unit's components.
//!
//! Every colour is unit's theme (`--unit-*`) or one of the few the editor adds
//! for a value's kind (`--fl-*`), set for both appearances the way unit sets
//! its own: so the editor is light or dark as the [`unit::UnitRoot`] it is
//! under says.
//!
//! Two things are drawn by hand rather than with a unit control, both for
//! the same reason: the editor reads its keys from the window, and a page
//! under Blitz cannot give an `<input>` the focus from code. So an open value
//! is the model's own buffer drawn as a field, with a caret, and the picker's
//! filter the same; unit's `TextField` would never get the keys typed into
//! it.

use dioxus::prelude::*;
use fig::Value;
use flower_core::{
    Backend, ChoiceTarget, EditSlot, FieldRuleExt, Mode, Model, NoticeKind, PageItem, Seg, VKind,
};
use unit::icon::paths;
use unit::{
    Accessory, Button, ContextMenu, Entry as MenuEntry, Item, MenuButton, Modal, Row, Section,
    Switch, Variant,
};

use crate::editor::{Editor, Geometry, adds_here, fold_held_open, notice_under};
use crate::layout::{self, Entry, Role, Section as Laid, Shown, Sizes};

/// The picker's most rows at once.
const CHOICES_SHOWN: usize = 12;

/// The editor's own colours: a value's kind, and the cursor's row. Both
/// appearances, keyed as unit keys its own palette, so a forced appearance
/// on the root is honoured.
const PALETTE: &str = r#"
.flower { --fl-bool: #9a2fa3; --fl-num: #0b7a86; --fl-str: #2d6a2d; --fl-ext: #9a6a00;
          --fl-sel: rgba(59, 127, 240, 0.13); --fl-sel-quiet: rgba(128, 128, 128, 0.12); }
@media (prefers-color-scheme: dark) {
  .unit-root[data-appearance="system"] .flower {
    --fl-bool: #d58ae0; --fl-num: #5fc4cf; --fl-str: #8fd18f; --fl-ext: #e0b45a;
    --fl-sel: rgba(90, 150, 255, 0.22); --fl-sel-quiet: rgba(160, 160, 160, 0.16); }
}
.unit-root[data-appearance="dark"] .flower {
    --fl-bool: #d58ae0; --fl-num: #5fc4cf; --fl-str: #8fd18f; --fl-ext: #e0b45a;
    --fl-sel: rgba(90, 150, 255, 0.22); --fl-sel-quiet: rgba(160, 160, 160, 0.16); }
"#;

/// The editor's stylesheet, at the sizes `z`.
///
/// Every row's lines are fixed — no wrapping, an ellipsis where a line runs
/// out — because the layout adds the heights up ([`layout::place`]) and
/// Blitz cannot be asked what they came to. unit's own sizes are set on the
/// editor's box from the same numbers, so a zoom scales its controls too.
pub fn css(z: &Sizes) -> String {
    let Sizes {
        font,
        pane_pad,
        gap,
        caption,
        caption_gap,
        row,
        subline,
        group_row,
        pane_title,
        action_row,
        bar,
        ..
    } = *z;
    let title_line = row - 16.0;
    let control = (24.0 * font / 13.0).round();
    format!(
        "{PALETTE}
html, body {{ margin: 0; overflow: hidden; }}
.flower {{ position: relative; overflow: hidden; display: flex; flex-direction: column;
           --unit-font-size: {font}px; --unit-control-height: {control}px; font-size: {font}px; }}
.flower .fl-bar {{ flex: none; display: flex; align-items: center; gap: 2px; height: {bar}px;
                   padding: 0 8px; border-bottom: 1px solid var(--unit-separator);
                   background: var(--unit-surface); white-space: nowrap; overflow: hidden; }}
.flower .fl-crumbs {{ display: flex; align-items: center; flex: 1; min-width: 0; overflow: hidden; }}
.flower .fl-crumb-sep {{ color: var(--unit-dim); padding: 0 1px; }}
.flower .fl-crumb-here {{ font-weight: 600; padding: 0 8px; overflow: hidden; text-overflow: ellipsis; }}
.flower .fl-bar-end {{ display: flex; align-items: center; gap: 6px; flex: none; }}
.flower .fl-panes {{ display: flex; flex: none; }}
.flower .fl-pane {{ position: relative; flex: none; overflow: hidden; background: var(--unit-bg); }}
.flower .fl-pane + .fl-pane {{ border-left: 1px solid var(--unit-separator); }}
.flower .fl-column {{ position: relative; display: flex; flex-direction: column; gap: {gap}px;
                      padding: {pane_pad}px 0; margin: 0 auto; }}
.flower .fl-pane.fl-peek .fl-column {{ opacity: 0.55; }}
.flower .fl-pane-title, .flower .unit-section-title {{ height: {caption}px; line-height: {caption}px;
                      font-size: 0.85em; font-weight: 600; color: var(--unit-dim); padding: 0 14px;
                      white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }}
.flower .fl-pane-title {{ height: {pane_title}px; line-height: {pane_title}px; }}
.flower .unit-section {{ gap: {caption_gap}px; }}
.flower .unit-section.fl-sel-caption .unit-section-title {{ color: var(--unit-accent); }}
.flower .unit-section-body > .fl-entry {{ padding: 0; }}
.flower .fl-entry {{ cursor: default; }}
.flower .fl-entry.fl-sel {{ background: var(--fl-sel); }}
.flower .fl-pane.fl-other .fl-entry.fl-sel {{ background: var(--fl-sel-quiet); }}
.flower .unit-row {{ min-height: 0; height: {row}px; padding: 0 14px; gap: 10px; }}
.flower .fl-sub1 .unit-row {{ height: {h1}px; }}
.flower .fl-sub2 .unit-row {{ height: {h2}px; }}
.flower .unit-row.unit-focused {{ outline: none; }}
.flower .unit-row-title {{ height: {title_line}px; line-height: {title_line}px; white-space: nowrap;
                           overflow: hidden; text-overflow: ellipsis; }}
.flower .unit-row-subtitle, .flower .unit-row-note {{ height: {subline}px; line-height: {subline}px;
                           margin: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }}
.flower .unit-row-detail {{ max-width: 55%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }}
.flower .unit-row-trailing {{ align-items: center; min-width: 0; overflow: hidden; }}
.flower .fl-said-error .unit-row-subtitle {{ color: var(--unit-danger); }}
.flower .fl-said-warning .unit-row-subtitle {{ color: var(--unit-warning); }}
.flower .fl-said-info .unit-row-subtitle {{ color: var(--unit-dim); }}
.flower .fl-index {{ color: var(--unit-dim); margin-right: 6px; }}
.flower .fl-tail {{ display: flex; align-items: center; gap: 8px; min-width: 0; white-space: nowrap; }}
.flower .fl-aside {{ color: var(--unit-dim); font-size: 0.9em; overflow: hidden; text-overflow: ellipsis;
                     flex: 0 1 auto; min-width: 0; }}
.flower .fl-value {{ overflow: hidden; text-overflow: ellipsis; flex: 0 1 auto; min-width: 0; }}
.flower .fl-v-null, .flower .fl-unset {{ color: var(--unit-dim); }}
.flower .fl-v-bool {{ color: var(--fl-bool); }}
.flower .fl-v-num {{ color: var(--fl-num); font-family: ui-monospace, Menlo, monospace; }}
.flower .fl-v-str {{ color: var(--fl-str); }}
.flower .fl-v-ext {{ color: var(--fl-ext); }}
.flower .fl-group {{ display: flex; align-items: center; gap: 8px; height: {group_row}px; padding: 0 14px;
                     font-size: 0.85em; font-weight: 600; color: var(--unit-dim); white-space: nowrap; }}
.flower .fl-group .fl-rule {{ flex: 1; height: 1px; background: var(--unit-separator); }}
.flower .fl-chips {{ display: flex; gap: 6px; align-items: center; overflow: hidden; white-space: nowrap; }}
.flower .fl-chip {{ display: flex; align-items: center; gap: 4px; padding: 2px 6px 2px 10px; border-radius: 11px;
                    background: var(--fl-sel); color: var(--unit-accent); font-weight: 500; }}
.flower .fl-chip.fl-sel {{ background: var(--unit-accent); color: var(--unit-accent-text); }}
.flower .fl-chip-x {{ padding: 0 3px; opacity: 0.7; }}
.flower .fl-chip-add {{ padding: 2px 10px; border-radius: 11px; color: var(--unit-dim);
                        border: 1px dashed var(--unit-border); }}
.flower .fl-field {{ display: flex; align-items: center; min-width: 80px; max-width: 100%; height: 22px; padding: 0 7px;
                     border-radius: 6px; border: 1.5px solid var(--unit-accent); background: var(--unit-control);
                     color: var(--unit-text); white-space: pre; overflow: hidden; justify-content: flex-end; }}
.flower .fl-field.fl-left {{ justify-content: flex-start; }}
.flower .fl-marked {{ text-decoration: underline; text-decoration-color: var(--unit-accent); }}
.flower .fl-caret {{ display: inline-block; width: 0; height: 15px; border-left: 2px solid var(--unit-accent);
                     margin-right: -2px; }}
.flower .fl-placeholder {{ color: var(--unit-dim); }}
.flower .fl-action {{ display: flex; align-items: center; gap: 6px; height: {action_row}px; padding: 0 14px;
                      color: var(--unit-accent); }}
.flower .fl-fold {{ color: var(--unit-dim); font-size: 0.85em; font-weight: 600; }}
.flower .fl-empty {{ padding: 12px 14px; color: var(--unit-dim); }}
.flower .fl-placeholder-pane {{ display: flex; align-items: center; justify-content: center; color: var(--unit-dim); }}
.fl-picker .fl-filter {{ margin-bottom: 8px; }}
.fl-picker .fl-choice {{ display: flex; gap: 10px; height: 26px; line-height: 26px; padding: 0 8px;
                         border-radius: 5px; white-space: nowrap; overflow: hidden; }}
.fl-picker .fl-choice.fl-sel {{ background: var(--unit-accent); color: var(--unit-accent-text); }}
.fl-picker .fl-choice .fl-detail {{ color: var(--unit-dim); overflow: hidden; text-overflow: ellipsis; }}
.fl-picker .fl-choice.fl-sel .fl-detail {{ color: var(--unit-accent-text); }}
.fl-picker .fl-field {{ display: flex; align-items: center; height: 26px; padding: 0 8px; border-radius: 6px;
                        border: 1.5px solid var(--unit-accent); background: var(--unit-control); white-space: pre; }}
.fl-picker .fl-caret {{ display: inline-block; width: 0; height: 15px; border-left: 2px solid var(--unit-accent); }}
.fl-picker .fl-placeholder, .fl-picker .fl-none {{ color: var(--unit-dim); }}
",
        h1 = row + subline,
        h2 = row + 2.0 * subline,
    )
}

impl<B: Backend + 'static> Editor<B> {
    /// The editor's part of the page: its stylesheet, the breadcrumb's bar
    /// with `trailing` — the host's own buttons — at its end, the panes and
    /// the picker. A host draws its own chrome beside it and says how much of
    /// the window that takes ([`Editor::set_bounds`]).
    pub fn render(&self, trailing: Option<Element>) -> Element {
        render(self, trailing)
    }
}

/// The editor as a component, for the fig-backed model: what a host puts
/// in its page, under a [`unit::UnitRoot`].
#[component]
pub fn FlowerPages(editor: Editor, trailing: Option<Element>) -> Element {
    editor.render(trailing)
}

fn render<B: Backend + 'static>(ed: &Editor<B>, trailing: Option<Element>) -> Element {
    // Read, so a change to any of them draws the page again.
    let _ = *ed.sig.tick.read();
    let _ = *ed.sig.size.read();
    let _ = *ed.sig.bounds.read();
    let look = ed.sig.look.read().clone();
    let g = ed.geometry();
    let model = ed.st.model.borrow();
    let shown = layout::panes(&*model, g.two);
    let offsets: Vec<f64> = ed.st.scroll.borrow().iter().map(|s| s.offset).collect();
    let picking = matches!(model.mode, Mode::Choosing { .. });

    rsx! {
        style { {css(&g.sizes)} }
        div { class: "flower", style: "width: {g.width}px; height: {g.height}px",
            {bar(ed, &model, &look.root, trailing)}
            div { class: "fl-panes", style: "height: {g.pane_height}px",
                for (slot, pane) in shown.into_iter().enumerate() {
                    match pane {
                        Some(s) => pane_view(ed, &model, s, slot, &g, offsets.get(slot).copied().unwrap_or(0.0)),
                        None => rsx! {
                            div { key: "{slot}", class: "fl-pane fl-placeholder-pane",
                                style: "width: {g.pane_width}px; height: {g.pane_height}px",
                                "Nothing to open here"
                            }
                        },
                    }
                }
            }
            {picker(ed, &model, picking)}
        }
    }
}

// ── the bar ─────────────────────────────────────────────────────────────────

/// The trail from the root to the page the cursor is on, each step a button
/// that opens it, and a back button before it — the one piece of chrome that
/// says where you are.
fn bar<B: Backend + 'static>(
    ed: &Editor<B>,
    model: &Model<B>,
    root: &str,
    trailing: Option<Element>,
) -> Element {
    let crumbs = layout::crumbs(model.page());
    let at_root = crumbs.is_empty();
    let back = ed.clone();
    let home = ed.clone();
    let n = crumbs.len();
    rsx! {
        div { class: "fl-bar", role: "toolbar", "aria-label": "Breadcrumb",
            Button { variant: Variant::Ghost, class: "unit-icon", icon: paths::CHEVRON_LEFT,
                     label: "Back", disabled: at_root, refuses_keys: true,
                     onpress: move |_| back.back() }
            div { class: "fl-crumbs",
                if at_root {
                    span { class: "fl-crumb-here", "{root}" }
                } else {
                    Button { variant: Variant::Ghost, refuses_keys: true,
                             onpress: move |_| home.open_page(Vec::new()), "{root}" }
                }
                for (k, (path, name)) in crumbs.into_iter().enumerate() {
                    span { key: "s{k}", class: "fl-crumb-sep", "›" }
                    if k + 1 == n {
                        span { key: "c{k}", class: "fl-crumb-here", "{name}" }
                    } else {
                        Button { key: "c{k}", variant: Variant::Ghost, refuses_keys: true,
                                 onpress: {
                                     let ed = ed.clone();
                                     move |_| ed.open_page(path.clone())
                                 },
                                 "{name}" }
                    }
                }
            }
            div { class: "fl-bar-end", {trailing} }
        }
    }
}

// ── a pane ──────────────────────────────────────────────────────────────────

/// One pane: its page's sections, the add row, and the fold, scrolled to
/// `offset`.
fn pane_view<B: Backend + 'static>(
    ed: &Editor<B>,
    model: &Model<B>,
    s: Shown<'_>,
    slot: usize,
    g: &Geometry,
    offset: f64,
) -> Element {
    let laid = layout::lay_out(&s.page, g.column(), &g.sizes);
    let verbatim = layout::verbatim(&s.page);
    let open = ed.st.advanced.borrow().contains(&s.page.focus) || fold_held_open(model, &laid, &s);
    let adds = s.role == Role::Current && adds_here(model, &s.page.focus);
    let class = match s.role {
        Role::Current => "fl-pane",
        Role::Parent => "fl-pane fl-other",
        Role::Peek => "fl-pane fl-other fl-peek",
    };
    let title = match layout::crumbs(&s.page).pop() {
        Some((_, name)) => name,
        None => ed.sig.look.peek().root.clone(),
    };
    let ctx = Ctx {
        ed,
        model,
        page: &s.page,
        role: s.role,
        selected: s.selected,
        verbatim: &verbatim,
        sizes: &g.sizes,
    };
    let focus = s.page.focus.clone();
    let fold = ed.clone();
    rsx! {
        div { key: "{slot}", class,
            style: "width: {g.pane_width}px; height: {g.pane_height}px",
            "aria-hidden": (s.role == Role::Peek).then_some("true"),
            div { class: "fl-column", style: "width: {g.column()}px; top: {-offset}px",
                if s.role != Role::Current {
                    div { class: "fl-pane-title", role: "heading", "aria-level": "2", "{title}" }
                }
                if s.page.is_empty() {
                    div { class: "fl-empty", "Empty" }
                }
                for (k, section) in laid.sections.iter().enumerate() {
                    {section_view(&ctx, section, k)}
                }
                if adds {
                    {add_row(&ctx)}
                }
                if laid.folded > 0 {
                    div { class: "fl-action fl-fold", role: "button",
                        "aria-expanded": if open { "true" } else { "false" },
                        onclick: move |e: MouseEvent| {
                            e.stop_propagation();
                            fold.toggle_advanced(focus.clone());
                        },
                        unit::Icon { d: if open { paths::CHEVRON_DOWN } else { paths::CHEVRON_RIGHT },
                                     ink: unit::Ink::Dim, size: 12.0 }
                        if open { "ADVANCED" } else { "ADVANCED  {laid.folded}" }
                    }
                    if open {
                        for (k, section) in laid.advanced.iter().enumerate() {
                            {section_view(&ctx, section, 1000 + k)}
                        }
                    }
                }
            }
        }
    }
}

/// What every row of a pane is drawn from.
struct Ctx<'a, B: Backend + 'static> {
    ed: &'a Editor<B>,
    model: &'a Model<B>,
    page: &'a flower_core::Page,
    role: Role,
    selected: Option<usize>,
    verbatim: &'a std::collections::HashSet<usize>,
    sizes: &'a Sizes,
}

impl<B: Backend + 'static> Ctx<'_, B> {
    fn is_selected(&self, index: usize) -> bool {
        self.selected == Some(index)
    }

    /// Whether the pane is the one being edited: a preview is a picture of
    /// somewhere the cursor is not, and offers nothing to act on.
    fn live(&self) -> bool {
        self.role != Role::Peek
    }
}

fn section_view<B: Backend + 'static>(c: &Ctx<'_, B>, section: &Laid<'_>, k: usize) -> Element {
    let title = section.header.map(|(_, item)| layout::group_name(item));
    let class = match section.header {
        Some((i, _)) if c.is_selected(i) => "fl-sel-caption",
        _ => "",
    };
    rsx! {
        Section { key: "{k}", title, class,
            for entry in section.entries.iter() {
                {entry_view(c, entry)}
            }
        }
    }
}

/// One entry of a section, in its context menu.
fn entry_view<B: Backend + 'static>(c: &Ctx<'_, B>, entry: &Entry<'_>) -> Element {
    let index = entry.index();
    let item = entry.item();
    let sel = match entry {
        Entry::Chips { .. } => c.selected.is_some_and(|s| entry.holds(s)),
        _ => c.is_selected(index),
    };
    let sub = c.ed.sublines(c.model, item);
    let class = format!(
        "fl-entry{}{}",
        if sel { " fl-sel" } else { "" },
        match sub {
            0 => "",
            1 => " fl-sub1",
            _ => " fl-sub2",
        }
    );
    let (entries, picks) = row_menu(c, item, index);
    let ed = c.ed.clone();
    let role = c.role;
    let body = match entry {
        Entry::Group { item, inset, .. } => rsx! {
            div { class: "fl-group", style: "padding-left: {14.0 + *inset as f64 * c.sizes.indent}px",
                span { {layout::group_name(item).to_uppercase()} }
                span { class: "fl-rule" }
            }
        },
        Entry::Row { item, inset, .. } => row_view(c, item, index, *inset),
        Entry::Chips {
            header,
            members,
            inset,
            ..
        } => chips_view(c, header, members, *inset),
    };
    let live = c.live();
    rsx! {
        ContextMenu { key: "{index}", class, entries,
            onpick: move |id: usize| if live { picks.run(&ed, id) },
            div { role: "button",
                onclick: {
                    let ed = c.ed.clone();
                    move |e: MouseEvent| {
                        e.stop_propagation();
                        ed.press(role, index);
                    }
                },
                {body}
            }
        }
    }
}

/// One item as a settings row: its name on the left, its value or control on
/// the right, and under the name what was said about it.
fn row_view<B: Backend + 'static>(
    c: &Ctx<'_, B>,
    item: &PageItem,
    index: usize,
    inset: usize,
) -> Element {
    let name = layout::row_name(item, c.verbatim.contains(&index));
    let title = match &item.title {
        Some(t) => format!("{name}  {t}"),
        None => name,
    };
    let (subtitle, said) = said_under(c.model, item);
    let note = leading_note(c, item);
    let accessory = if item.is_drill() {
        Accessory::Chevron
    } else {
        Accessory::None
    };
    let trailing = tail(c, item);
    rsx! {
        Row { title, subtitle, note, accessory, depth: inset, class: said, trailing }
    }
}

/// What is said under a row's name, and the class that colours it: a
/// refusal or a warning about the row, else the host's finding.
fn said_under<B: Backend>(model: &Model<B>, item: &PageItem) -> (Option<String>, String) {
    if let Some(n) = notice_under(model, &item.path) {
        let class = match n.kind {
            NoticeKind::Rejected => "fl-said-error",
            _ => "fl-said-warning",
        };
        return (Some(n.message.clone()), class.into());
    }
    match &item.annotation {
        Some(a) => {
            let class = match a.severity {
                flower_core::annotate::Severity::Error => "fl-said-error",
                flower_core::annotate::Severity::Warning => "fl-said-warning",
                flower_core::annotate::Severity::Info => "fl-said-info",
            };
            (Some(a.message.clone()), class.into())
        }
        None => (None, String::new()),
    }
}

/// The line under a row: the comment written above it in the file, its first
/// line — or, while it is being typed, the comment's field.
fn leading_note<B: Backend>(c: &Ctx<'_, B>, item: &PageItem) -> Option<String> {
    if let Mode::Editing {
        path,
        slot: EditSlot::LeadingComment,
        buffer,
    } = &c.model.mode
        && *path == item.path
    {
        let marked = c.ed.st.marked.borrow();
        return Some(format!("# {}{}▏", buffer.replace('\n', " ⏎ "), marked));
    }
    item.leading_comment
        .as_ref()
        .map(|t| t.lines().next().unwrap_or_default().to_string())
}

/// The right of a row: an open field, a switch, a vocabulary's menu, a
/// container's summary, or the value and the comment after it.
fn tail<B: Backend + 'static>(c: &Ctx<'_, B>, item: &PageItem) -> Option<Element> {
    if let Mode::Editing { path, slot, buffer } = &c.model.mode
        && *path == item.path
        && *slot != EditSlot::LeadingComment
    {
        let placeholder = match slot {
            EditSlot::NewItem => "new item",
            EditSlot::TrailingComment => "comment",
            _ => "value",
        };
        let prefix = if *slot == EditSlot::TrailingComment {
            "# "
        } else {
            ""
        };
        return Some(field(c, &format!("{prefix}{buffer}"), placeholder, false));
    }
    let aside = item
        .trailing_comment
        .as_ref()
        .filter(|t| !t.is_empty())
        .map(|t| format!("# {t}"));
    if !item.is_scalar() {
        let summary = layout::drill_summary(item);
        return Some(rsx! {
            span { class: "fl-tail",
                if let Some(a) = aside { span { class: "fl-aside", "{a}" } }
                span { class: "fl-value fl-unset", "{summary}" }
            }
        });
    }
    let path = item.path.clone();
    if item.vkind == VKind::Bool && c.live() {
        let on = item.preview == "true";
        let ed = c.ed.clone();
        let label = layout::row_name(item, false);
        return Some(rsx! {
            span { class: "fl-tail",
                if let Some(a) = aside { span { class: "fl-aside", "{a}" } }
                span { onclick: |e: MouseEvent| e.stop_propagation(),
                    Switch { on, onchange: move |v| ed.set_bool(path.clone(), v),
                             span { class: "fl-hidden-label", "aria-label": "{label}" } }
                }
            }
        });
    }
    if c.live()
        && let Some(choices) = c.model.choices_at(&item.path)
    {
        let closed = c
            .model
            .rule_at(&item.path)
            .and_then(|r| r.enum_constraint())
            .is_some_and(|(_, closed)| closed);
        let current = choices.iter().position(|ch| ch.label == item.preview);
        let mut entries: Vec<MenuEntry> = choices
            .iter()
            .enumerate()
            .map(|(i, ch)| {
                Item::new(i, ch.label.clone())
                    .checked(current == Some(i))
                    .into()
            })
            .collect();
        let other = choices.len();
        if !closed {
            entries.push(MenuEntry::Separator);
            entries.push(Item::new(other, "Other…").into());
        }
        let unlisted = !item.preview.is_empty() && current.is_none();
        let shown = if item.preview.is_empty() {
            "Not set".to_string()
        } else {
            item.preview.clone()
        };
        let class = if unlisted && closed {
            "fl-value fl-said-warning"
        } else {
            "fl-value"
        };
        let ed = c.ed.clone();
        let label = layout::row_name(item, false);
        return Some(rsx! {
            span { onclick: |e: MouseEvent| e.stop_propagation(),
                MenuButton { class: "unit-select", entries, refuses_keys: true, label: "{label}: {shown}",
                    onpick: move |i: usize| {
                        if i == other {
                            ed.edit(path.clone(), EditSlot::Value);
                        } else {
                            ed.set_choice(path.clone(), i);
                        }
                    },
                    span { class, "{shown}" }
                    span { class: "unit-select-arrows", "aria-hidden": "true", "▾" }
                }
            }
        });
    }
    let (value, class) = if item.preview.is_empty() {
        ("Not set".to_string(), "fl-value fl-unset".to_string())
    } else {
        (
            item.preview.clone(),
            format!("fl-value {}", kind_class(item.vkind)),
        )
    };
    Some(rsx! {
        span { class: "fl-tail",
            if let Some(a) = aside { span { class: "fl-aside", "{a}" } }
            span { class, "{value}" }
        }
    })
}

/// The class a value's kind is drawn in.
pub fn kind_class(k: VKind) -> &'static str {
    match k {
        VKind::Null => "fl-v-null",
        VKind::Bool => "fl-v-bool",
        VKind::Int | VKind::Float => "fl-v-num",
        VKind::Str => "fl-v-str",
        VKind::Ext => "fl-v-ext",
        VKind::Map | VKind::Seq => "fl-unset",
    }
}

/// The open field: the model's buffer, the input method's composition
/// underlined after it, and the caret. A press inside it is the field's.
fn field<B: Backend>(c: &Ctx<'_, B>, text: &str, placeholder: &str, left: bool) -> Element {
    let marked = c.ed.st.marked.borrow().clone();
    let shown = text.replace('\n', "⏎");
    let class = if left { "fl-field fl-left" } else { "fl-field" };
    let empty = shown.is_empty() && marked.is_empty();
    rsx! {
        span { class, role: "textbox", "aria-label": "{placeholder}", "aria-valuetext": "{shown}",
            onclick: |e: MouseEvent| e.stop_propagation(),
            if empty {
                span { class: "fl-placeholder", "{placeholder}" }
            }
            span { "{shown}" }
            span { class: "fl-marked", "{marked}" }
            span { class: "fl-caret" }
        }
    }
}

/// A scalar list as one row: its name, and each member a chip — pressed to
/// edit it, its × to delete it — with a chip at the end that adds one.
fn chips_view<B: Backend + 'static>(
    c: &Ctx<'_, B>,
    header: &PageItem,
    members: &[(usize, &PageItem)],
    inset: usize,
) -> Element {
    let title = layout::row_name(header, false);
    let (subtitle, said) = said_under(c.model, header);
    let note = leading_note(c, header);
    let editing = match &c.model.mode {
        Mode::Editing {
            path, slot, buffer, ..
        } => Some((path.clone(), *slot, buffer.clone())),
        _ => None,
    };
    let live = c.live();
    let adding = editing
        .as_ref()
        .is_some_and(|(p, s, _)| *p == header.path && *s == EditSlot::NewItem);
    let list = header.path.clone();
    let add = c.ed.clone();
    let trailing = rsx! {
        div { class: "fl-chips",
            for (i, m) in members.iter() {
                {
                    let open = editing.as_ref().filter(|(p, s, _)| *p == m.path && *s == EditSlot::Value);
                    match open {
                        Some((_, _, buffer)) => field(c, buffer, "value", true),
                        None => chip(c, *i, m, live),
                    }
                }
            }
            if adding {
                {field(c, &editing.as_ref().map(|e| e.2.clone()).unwrap_or_default(), "new item", true)}
            } else if live {
                span { class: "fl-chip-add", role: "button", "aria-label": "Add to {title}",
                    onclick: move |e: MouseEvent| {
                        e.stop_propagation();
                        add.add_item(list.clone());
                    },
                    "+"
                }
            }
        }
    };
    rsx! {
        Row { title: title.clone(), subtitle, note, depth: inset, class: said, trailing }
    }
}

fn chip<B: Backend + 'static>(c: &Ctx<'_, B>, index: usize, m: &PageItem, live: bool) -> Element {
    let text = if m.preview.is_empty() {
        "—".to_string()
    } else {
        m.preview.clone()
    };
    let class = if c.is_selected(index) {
        "fl-chip fl-sel"
    } else {
        "fl-chip"
    };
    let (edit, gone) = (c.ed.clone(), c.ed.clone());
    let (path, del) = (m.path.clone(), m.path.clone());
    let role = c.role;
    rsx! {
        span { key: "{index}", class, role: "button", "aria-label": "{text}",
            onclick: move |e: MouseEvent| {
                e.stop_propagation();
                if role == Role::Current {
                    edit.edit(path.clone(), EditSlot::Value);
                } else {
                    edit.press(role, index);
                }
            },
            "{text}"
            if live {
                span { class: "fl-chip-x", role: "button", "aria-label": "Delete {text}",
                    onclick: move |e: MouseEvent| {
                        e.stop_propagation();
                        gone.delete(del.clone());
                    },
                    "×"
                }
            }
        }
    }
}

/// The page's own offer to add: a field to a mapping — the schema's
/// declared ones by name, and any key — or an item to a list.
fn add_row<B: Backend + 'static>(c: &Ctx<'_, B>) -> Element {
    let focus = c.page.focus.clone();
    let is_seq = matches!(c.model.value_at(&focus), Some(Value::Seq(_)));
    let ed = c.ed.clone();
    if is_seq {
        let adding = matches!(&c.model.mode, Mode::Editing { path, slot: EditSlot::NewItem, .. } if *path == focus);
        if adding {
            let buffer = match &c.model.mode {
                Mode::Editing { buffer, .. } => buffer.clone(),
                _ => String::new(),
            };
            return rsx! {
                div { class: "fl-action", {field(c, &buffer, "new item", true)} }
            };
        }
        return rsx! {
            div { class: "fl-action", role: "button",
                onclick: move |e: MouseEvent| {
                    e.stop_propagation();
                    ed.add_item(focus.clone());
                },
                unit::Icon { d: paths::PLUS, ink: unit::Ink::Accent, size: 13.0 }
                "Add an Item"
            }
        };
    }
    // The fields the schema declares that the document has not got yet: at
    // the document's root, where a schema names them.
    let declared: Vec<String> = if focus.is_empty() {
        c.model
            .addable_fields()
            .iter()
            .filter_map(|r| match r.at.0.as_slice() {
                [flower_core::SegPat::Key(k)] => Some(k.clone()),
                _ => None,
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut entries: Vec<MenuEntry> = declared
        .iter()
        .enumerate()
        .map(|(i, k)| Item::new(i, layout::prettify(k)).into())
        .collect();
    if !entries.is_empty() {
        entries.push(MenuEntry::Separator);
    }
    let custom = declared.len();
    entries.push(Item::new(custom, "Custom Field…").into());
    rsx! {
        div { class: "fl-action",
            MenuButton { class: "unit-ghost", entries, refuses_keys: true, icon: paths::PLUS,
                label: "Add a Field",
                onpick: move |i: usize| {
                    ed.add_field(focus.clone(), declared.get(i).cloned());
                },
                "Add a Field…"
            }
        }
    }
}

// ── the row's menu ──────────────────────────────────────────────────────────

/// What a row's context menu does, by item id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Open,
    Edit,
    Choose,
    Comment,
    CommentAbove,
    Rename,
    AddField,
    AddItem,
    MoveUp,
    MoveDown,
    Delete,
}

const COMMANDS: [Command; 11] = [
    Command::Open,
    Command::Edit,
    Command::Choose,
    Command::Comment,
    Command::CommentAbove,
    Command::Rename,
    Command::AddField,
    Command::AddItem,
    Command::MoveUp,
    Command::MoveDown,
    Command::Delete,
];

/// What a picked id does, for the row at `path` on the pane drawn as `role`.
#[derive(Clone)]
struct Picks {
    path: Vec<Seg>,
    role: Role,
    index: usize,
}

impl Picks {
    fn run<B: Backend + 'static>(&self, ed: &Editor<B>, id: usize) {
        let path = self.path.clone();
        match COMMANDS.get(id) {
            Some(Command::Open) => ed.press(self.role, self.index),
            Some(Command::Edit) => ed.edit(path, EditSlot::Value),
            Some(Command::Choose) => ed.choose(path),
            Some(Command::Comment) => ed.edit(path, EditSlot::TrailingComment),
            Some(Command::CommentAbove) => ed.edit(path, EditSlot::LeadingComment),
            Some(Command::Rename) => ed.rename(path),
            Some(Command::AddField) => ed.add_field(path, None),
            Some(Command::AddItem) => ed.add_item(path),
            Some(Command::MoveUp) => ed.move_by(path, -1),
            Some(Command::MoveDown) => ed.move_by(path, 1),
            Some(Command::Delete) => ed.delete(path),
            None => {}
        }
    }
}

/// A row's context menu: every operation takes a path, and a group header
/// still carries one, so a group inlined into this page is as operable as a
/// row that opens its own.
fn row_menu<B: Backend>(c: &Ctx<'_, B>, item: &PageItem, index: usize) -> (Vec<MenuEntry>, Picks) {
    let id = |cmd: Command| COMMANDS.iter().position(|c| *c == cmd).unwrap();
    let mut e: Vec<MenuEntry> = Vec::new();
    let mut add = |cmd: Command, label: &str, enabled: bool| {
        let mut it = Item::new(id(cmd), label);
        it.enabled = enabled;
        e.push(it.into());
    };
    let choices = c.model.choices_at(&item.path).is_some();
    if item.is_drill() {
        add(Command::Open, "Open", true);
    }
    if item.is_scalar() {
        add(Command::Edit, "Edit Value", true);
        if choices {
            add(Command::Choose, "Choose…", true);
        }
    }
    add(Command::Comment, "Edit Comment", true);
    add(Command::CommentAbove, "Edit Comment Above", true);
    if item.can_rename() {
        add(Command::Rename, "Rename…", true);
    }
    match c.model.value_at(&item.path) {
        Some(Value::Map(_)) => add(Command::AddField, "Add Field…", true),
        Some(Value::Seq(_)) => add(Command::AddItem, "Add Item", true),
        _ => {}
    }
    e.push(MenuEntry::Separator);
    let mut add = |cmd: Command, label: &str, enabled: bool| {
        let mut it = Item::new(id(cmd), label);
        it.enabled = enabled;
        e.push(it.into());
    };
    add(
        Command::MoveUp,
        "Move Up",
        layout::can_move(c.page, index, -1),
    );
    add(
        Command::MoveDown,
        "Move Down",
        layout::can_move(c.page, index, 1),
    );
    e.push(MenuEntry::Separator);
    let mut del = Item::new(id(Command::Delete), "Delete");
    del.destructive = true;
    e.push(del.into());
    (
        e,
        Picks {
            path: item.path.clone(),
            role: c.role,
            index,
        },
    )
}

// ── the picker ──────────────────────────────────────────────────────────────

/// The picker: a modal over the page, its filter typed into a field at the
/// top, the choices scrolled only as far as the cursor needs. Always in the
/// page, and shown while the model is choosing.
fn picker<B: Backend + 'static>(ed: &Editor<B>, model: &Model<B>, open: bool) -> Element {
    let (selected, filter, adding) = match &model.mode {
        Mode::Choosing {
            selected,
            filter,
            target,
            ..
        } => (
            *selected,
            filter.clone(),
            matches!(target, ChoiceTarget::Append(_)),
        ),
        _ => (0, String::new(), false),
    };
    let title = if adding {
        "Add an Item"
    } else {
        "Choose a Value"
    };
    let choices = if open {
        model.visible_choices()
    } else {
        Vec::new()
    };
    let first = selected.saturating_sub(CHOICES_SHOWN - 1);
    let rows: Vec<(usize, String, Option<String>, bool)> = choices
        .iter()
        .enumerate()
        .skip(first)
        .take(CHOICES_SHOWN)
        .map(|(i, c)| (i, c.label.clone(), c.detail.clone(), i == selected))
        .collect();
    let empty = open && choices.is_empty();
    let marked = ed.st.marked.borrow().clone();
    let close = ed.clone();
    rsx! {
        Modal { open, title, class: "fl-picker", width: 380.0,
            onclose: move |_| close.dismiss(),
            div { class: "fl-filter",
                div { class: "fl-field", role: "searchbox", "aria-label": "Filter",
                    if filter.is_empty() && marked.is_empty() {
                        span { class: "fl-placeholder", "Type to filter" }
                    }
                    span { "{filter}" }
                    span { class: "fl-marked", "{marked}" }
                    span { class: "fl-caret" }
                }
            }
            div { role: "listbox", "aria-label": "{title}",
                if empty {
                    div { class: "fl-choice fl-none", "Nothing matches" }
                }
                for (i, label, detail, sel) in rows {
                    div { key: "{i}", class: if sel { "fl-choice fl-sel" } else { "fl-choice" },
                        role: "option", "aria-selected": if sel { "true" } else { "false" },
                        onclick: {
                            let ed = ed.clone();
                            move |e: MouseEvent| {
                                e.stop_propagation();
                                ed.choose_at(i);
                            }
                        },
                        span { "{label}" }
                        if let Some(d) = detail {
                            span { class: "fl-detail", "{d}" }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_editor_colour_is_set_for_both_appearances() {
        let all = css(&Sizes::BASE);
        for var in ["--fl-bool", "--fl-num", "--fl-str", "--fl-ext", "--fl-sel"] {
            assert_eq!(all.matches(&format!("{var}:")).count(), 3, "{var}");
        }
        // Every other colour is unit's.
        let rules = all.split("@media").last().unwrap();
        let rules = rules
            .split(".unit-root[data-appearance=\"dark\"] .flower {")
            .nth(1)
            .unwrap();
        let rules = rules.split_once('}').unwrap().1;
        assert!(
            !rules.contains('#'),
            "a literal colour outside the palettes"
        );
    }

    #[test]
    fn the_stylesheet_sets_the_heights_the_layout_adds_up() {
        let z = Sizes::at(1.5);
        let all = css(&z);
        assert!(all.contains(&format!("height: {}px", z.row)), "{all}");
        assert!(all.contains(&format!("height: {}px", z.row + z.subline)));
        assert!(all.contains(&format!("--unit-font-size: {}px", z.font)));
    }
}
