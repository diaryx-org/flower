//! The menu bar: Flower, File, Edit, Go, View and Window — through muda,
//! which builds the native `NSMenu` on macOS.
//!
//! Every item *is* a key: the chord it stands for, replayed through the same
//! handler the keyboard goes through (`Shell::chord`) — the editor's table
//! first, then the shell's. So a command is defined once, and the menu cannot
//! disagree with the keyboard. A chord with ⌘ or ⌥ is shown as the item's
//! shortcut. The editor's single letters (`e`, `a`, `c`…) are not: AppKit
//! hands a shortcut to the menu before the view sees the key, so an item
//! whose shortcut was `e` would take every `e` typed into a value. Those
//! items carry the letter only as the chord they replay.
//!
//! What an item can do depends on what the editor is doing, so each has a
//! [`Gate`], read off the model after every change ([`Enabled::of`]): the
//! letter items are off while a value is open — replayed then, `e` would be
//! typed — Paste is on only then, Undo and Redo follow the journal.
//!
//! The app menu's Quit, Hide and Services and the Window menu are AppKit's
//! own (`PredefinedMenuItem`): they act on the application, not the document.
//!
//! muda is built without GTK, so this compiles and is tested on Linux, where
//! the menu is made and simply not shown: every command is a key regardless.

use std::cell::Cell;
use std::collections::HashMap;
use std::str::FromStr;

use dioxus::html::{Code, Key, Modifiers, NamedKey};
use flower_core::{Backend, Mode, Model};
use muda::accelerator::Accelerator;
use muda::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};

/// A menu item's command: the chord it stands for.
#[derive(Clone, Debug, PartialEq)]
pub struct Chord(pub Key, pub Code, pub Modifiers);

/// When an item can be chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gate {
    Always,
    /// Nothing open: the page's own commands.
    Normal,
    /// Nothing open, and the cursor on an item.
    Item,
    /// Nothing open, and a list to add to.
    Add,
    Undo,
    Redo,
    /// A value or the picker open: what a paste goes into.
    Open,
}

/// What can be done now, read off the model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Enabled {
    normal: bool,
    item: bool,
    add: bool,
    undo: bool,
    redo: bool,
}

impl Enabled {
    pub fn of<B: Backend>(m: &Model<B>) -> Self {
        let normal = matches!(m.mode, Mode::Normal);
        Self {
            normal,
            item: normal && m.page_item().is_some(),
            add: normal && m.append_target().is_some(),
            undo: normal && m.history_len() > 0,
            redo: normal && m.redo_len() > 0,
        }
    }

    fn allows(self, gate: Gate) -> bool {
        match gate {
            Gate::Always => true,
            Gate::Normal => self.normal,
            Gate::Item => self.item,
            Gate::Add => self.add,
            Gate::Undo => self.undo,
            Gate::Redo => self.redo,
            Gate::Open => !self.normal,
        }
    }
}

/// The menu bar, kept alive for as long as the app, and what its items do.
pub struct AppMenu {
    _menu: Menu,
    commands: HashMap<MenuId, Chord>,
    gated: Vec<(MenuItem, Gate)>,
    /// What the items were last enabled for.
    shown: Cell<Option<Enabled>>,
}

impl AppMenu {
    /// The chord an item chosen since the last call stands for, if any.
    pub fn next(&self) -> Option<Chord> {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(c) = self.commands.get(&event.id) {
                return Some(c.clone());
            }
        }
        None
    }

    /// Enable each item for what can be done now; a no-op while that has
    /// not changed.
    pub fn enable(&self, now: Enabled) {
        if self.shown.replace(Some(now)) == Some(now) {
            return;
        }
        for (item, gate) in &self.gated {
            item.set_enabled(now.allows(*gate));
        }
    }
}

/// ⌘ in Dioxus's terms (`SUPER`, as winit reports it).
const CMD: Modifiers = Modifiers::META;
const SHIFT: Modifiers = Modifiers::SHIFT;
const ALT: Modifiers = Modifiers::ALT;
const NONE: Modifiers = Modifiers::empty();

fn ch(c: &str) -> Key {
    Key::Character(c.into())
}

/// Build the menu bar, and on macOS install it as the application's.
pub fn install() -> AppMenu {
    let mut b = Builder::default();

    let app = Submenu::with_items(
        "Flower",
        true,
        &[
            &PredefinedMenuItem::about(None, None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ],
    )
    .expect("the app menu");

    let file = Submenu::new("File", true);
    b.add(&file, "Open…", ch("o"), Code::KeyO, CMD, Gate::Always);
    sep(&file);
    b.add(&file, "Save", ch("s"), Code::KeyS, CMD, Gate::Always);
    b.add(
        &file,
        "Save As…",
        ch("S"),
        Code::KeyS,
        CMD | SHIFT,
        Gate::Always,
    );
    sep(&file);
    let _ = file.append(&PredefinedMenuItem::close_window(None));

    // Titled "Edit" so AppKit adds its own Emoji & Symbols and Start
    // Dictation items to it.
    let edit = Submenu::new("Edit", true);
    b.add(&edit, "Undo", ch("z"), Code::KeyZ, CMD, Gate::Undo);
    b.add(&edit, "Redo", ch("Z"), Code::KeyZ, CMD | SHIFT, Gate::Redo);
    sep(&edit);
    b.add(&edit, "Paste", ch("v"), Code::KeyV, CMD, Gate::Open);
    sep(&edit);
    b.add(&edit, "Edit Value", ch("e"), Code::KeyE, NONE, Gate::Item);
    b.add(
        &edit,
        "Edit as Text",
        ch("E"),
        Code::KeyE,
        SHIFT,
        Gate::Item,
    );
    b.add(&edit, "Add Item", ch("a"), Code::KeyA, NONE, Gate::Add);
    b.add(
        &edit,
        "Add Item as Text",
        ch("A"),
        Code::KeyA,
        SHIFT,
        Gate::Add,
    );
    sep(&edit);
    b.add(&edit, "Edit Comment", ch("c"), Code::KeyC, NONE, Gate::Item);
    b.add(
        &edit,
        "Edit Comment Above",
        ch("C"),
        Code::KeyC,
        SHIFT,
        Gate::Item,
    );
    sep(&edit);
    b.add(
        &edit,
        "Move Up",
        Key::Named(NamedKey::ArrowUp),
        Code::ArrowUp,
        ALT,
        Gate::Item,
    );
    b.add(
        &edit,
        "Move Down",
        Key::Named(NamedKey::ArrowDown),
        Code::ArrowDown,
        ALT,
        Gate::Item,
    );
    b.add(
        &edit,
        "Delete",
        Key::Named(NamedKey::Backspace),
        Code::Backspace,
        CMD,
        Gate::Item,
    );

    // The Finder's Go menu, in its words and on its chords.
    let go = Submenu::new("Go", true);
    b.add(
        &go,
        "Open",
        Key::Named(NamedKey::ArrowDown),
        Code::ArrowDown,
        CMD,
        Gate::Item,
    );
    b.add(
        &go,
        "Enclosing Page",
        Key::Named(NamedKey::ArrowUp),
        Code::ArrowUp,
        CMD,
        Gate::Normal,
    );
    sep(&go);
    b.add(&go, "Next Item", ch("j"), Code::KeyJ, NONE, Gate::Normal);
    b.add(
        &go,
        "Previous Item",
        ch("k"),
        Code::KeyK,
        NONE,
        Gate::Normal,
    );

    let view = Submenu::new("View", true);
    // Items of their own, not just keys: macOS hands the menu a chord it
    // names before the window sees it, which keeps Blitz's page zoom (on the
    // same three chords) from also acting.
    b.add(
        &view,
        "Make Text Bigger",
        ch("="),
        Code::Equal,
        CMD,
        Gate::Always,
    );
    b.add(
        &view,
        "Make Text Smaller",
        ch("-"),
        Code::Minus,
        CMD,
        Gate::Always,
    );
    b.add(
        &view,
        "Actual Size",
        ch("0"),
        Code::Digit0,
        CMD,
        Gate::Always,
    );
    sep(&view);
    let _ = view.append(&PredefinedMenuItem::fullscreen(None));

    let window = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::bring_all_to_front(None),
        ],
    )
    .expect("the window menu");

    let menu = Menu::with_items(&[&app, &file, &edit, &go, &view, &window]).expect("the menu bar");
    #[cfg(target_os = "macos")]
    {
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
    }
    AppMenu {
        _menu: menu,
        commands: b.commands,
        gated: b.gated,
        shown: Cell::new(None),
    }
}

fn sep(m: &Submenu) {
    let _ = m.append(&PredefinedMenuItem::separator());
}

#[derive(Default)]
struct Builder {
    commands: HashMap<MenuId, Chord>,
    gated: Vec<(MenuItem, Gate)>,
}

impl Builder {
    /// An item that presses `key` with `mods`, shown with it as the shortcut
    /// when the chord has ⌘ or ⌥ — see the module's doc for why a letter
    /// alone, or with ⇧, is not.
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        menu: &Submenu,
        label: &str,
        key: Key,
        code: Code,
        mods: Modifiers,
        gate: Gate,
    ) {
        let id = MenuId::new(format!("flower.{}", self.commands.len()));
        let accel = shortcut(mods).then(|| accelerator(mods, code)).flatten();
        let item = MenuItem::with_id(id.clone(), label, true, accel);
        let _ = menu.append(&item);
        self.commands.insert(id, Chord(key, code, mods));
        if gate != Gate::Always {
            self.gated.push((item, gate));
        }
    }
}

/// Whether a chord can be a menu shortcut without taking a key a value is
/// typed with.
fn shortcut(mods: Modifiers) -> bool {
    mods.intersects(CMD | ALT | Modifiers::CONTROL)
}

/// A chord as muda's accelerator. muda takes `keyboard-types` 0.8, whose key
/// codes share their names with Dioxus's, and calls ⌘ `META`.
fn accelerator(mods: Modifiers, code: Code) -> Option<Accelerator> {
    use muda::accelerator::{Code as MCode, Modifiers as MMods};
    let code = MCode::from_str(&format!("{code:?}")).ok()?;
    let mut m = MMods::empty();
    m.set(MMods::META, mods.contains(CMD));
    m.set(MMods::SHIFT, mods.contains(SHIFT));
    m.set(MMods::ALT, mods.contains(ALT));
    m.set(MMods::CONTROL, mods.contains(Modifiers::CONTROL));
    Some(Accelerator::new(m, code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flower_core::{FigBackend, Seg, ViewMode};
    use flower_dioxus::input::{Action, apply_key};

    fn model() -> Model<FigBackend> {
        let (mut m, _) = crate::file::open_or_sample(None);
        m.set_view(ViewMode::Pages);
        m
    }

    #[test]
    fn a_chord_with_command_is_its_items_shortcut_and_a_letter_never_is() {
        assert_eq!(
            accelerator(CMD | SHIFT, Code::KeyS),
            Some(Accelerator::from_str("Shift+Cmd+KeyS").unwrap())
        );
        assert!(shortcut(ALT));
        assert!(!shortcut(NONE));
        assert!(!shortcut(SHIFT));
    }

    /// Every item's chord is one the editor takes — or, for Open, the
    /// shell's — so the menu cannot offer what the keyboard does not do.
    #[test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "muda makes menu items on the main thread only"
    )]
    fn every_item_is_a_key_the_editor_takes() {
        let menu = install();
        assert!(menu.commands.len() > 15);
        for Chord(key, _, mods) in menu.commands.values() {
            let mut m = model();
            // On a list item, so moving, adding and opening all apply.
            m.focus_on(&[
                Seg::Key("server".into()),
                Seg::Key("tags".into()),
                Seg::Index(0),
            ]);
            let open = key == &ch("o") && *mods == CMD;
            let paste = key == &ch("v") && *mods == CMD;
            if paste {
                m.begin_edit();
            }
            let action = apply_key(&mut m, key, *mods);
            assert_eq!(
                action == Action::Ignored,
                open,
                "{key:?} {mods:?} -> {action:?}"
            );
        }
    }

    #[test]
    fn the_letter_items_are_off_while_a_value_is_open_and_paste_only_then() {
        let mut m = model();
        m.focus_on(&[Seg::Key("title".into())]);
        let normal = Enabled::of(&m);
        assert!(normal.allows(Gate::Item) && !normal.allows(Gate::Open));
        assert!(!normal.allows(Gate::Undo) && !normal.allows(Gate::Add));
        m.begin_edit();
        let open = Enabled::of(&m);
        assert!(!open.allows(Gate::Item) && !open.allows(Gate::Normal));
        assert!(open.allows(Gate::Open));
        m.edit_push('!');
        m.edit_commit();
        let after = Enabled::of(&m);
        assert!(after.allows(Gate::Undo) && !after.allows(Gate::Redo));
    }

    #[test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "muda makes menu items on the main thread only"
    )]
    fn enabling_for_the_same_state_twice_is_a_no_op() {
        let menu = install();
        let m = model();
        menu.enable(Enabled::of(&m));
        assert_eq!(menu.shown.get(), Some(Enabled::of(&m)));
        menu.enable(Enabled::of(&m));
        let undo = menu.gated.iter().find(|(_, g)| *g == Gate::Undo).unwrap();
        assert!(!undo.0.is_enabled());
    }
}
