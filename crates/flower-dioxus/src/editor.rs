//! The editor: the model, the host, and the few signals the page is drawn
//! from — and every way into it: keys, the window's events, clicks, the
//! wheel.
//!
//! An [`Editor`] is a cheap handle — shared state behind an `Rc`, and signals,
//! which are `Copy` — so every event source holds a clone and calls a method
//! on it. The page is redrawn from the model on every change ([`Editor::refresh`]
//! bumps a signal the render reads): a page is a screenful of rows, not a
//! document of thousands, so there is nothing to memoize.

use std::cell::{Cell, RefCell, RefMut};
use std::rc::Rc;
use std::sync::Arc;

use dioxus::html::{Code, Key, Modifiers};
use dioxus::prelude::*;
use dioxus_native::winit::event::{Ime, WindowEvent};
use dioxus_native::winit::window::{Theme, Window};
use flower_core::{Backend, FigBackend, Mode, Model, ViewMode};

use crate::host::Host;
use crate::input::{Action, apply_key, insert_text};
use crate::layout::Role;
use crate::view::Appearance;

/// Below this many CSS pixels the two panes leave neither one usable, so the
/// editor is one column — the Swift host's `twoPaneMinWidth`.
pub const TWO_PANE_MIN: f32 = 620.0;

/// The footer's height in CSS pixels — `.footer` in the stylesheet.
pub const FOOTER: f32 = 28.0;

/// The space above a pane's breadcrumb, in CSS pixels.
pub const TOP: f32 = 8.0;

/// A pane's padding on each side, in CSS pixels.
pub const PAD: f32 = 12.0;

/// How wide a cell of the monospace face is, in ems. The common system
/// monospace faces (SF Mono, Menlo, DejaVu Sans Mono, Consolas) are all
/// within a hair of 0.6; a touch over it errs toward cutting a value one
/// character early rather than letting it run into the next pane.
pub const CELL_EM: f32 = 0.61;

/// What the editor draws with: the host's display choices.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// The monospace face, as a CSS family list. Monospace because a page is
    /// laid out in cells, so names read as one column and values as another,
    /// the way the file's own `key = value` lines up.
    pub mono: String,
    /// The type's size in CSS pixels.
    pub size: f32,
    /// Light or dark: the system's, or one of the two.
    pub appearance: Appearance,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            mono: "ui-monospace, \"SF Mono\", Menlo, \"DejaVu Sans Mono\", Consolas, monospace"
                .into(),
            size: 14.0,
            appearance: Appearance::System,
        }
    }
}

impl Look {
    /// A line's height in CSS pixels, whole so lines stack without seams.
    pub fn line(&self) -> f32 {
        (self.size * 1.6).round()
    }

    /// A cell's width in CSS pixels.
    pub fn cell(&self) -> f32 {
        self.size * CELL_EM
    }
}

/// Where everything goes in a window `width` × `height` CSS pixels, for a
/// look: how many panes there is room for, how many cells wide each is, and
/// how many lines of items each has. Arithmetic on the stylesheet's
/// constants rather than a measurement, because the model's inline budget has
/// to be sized *before* the frame it applies to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// Whether the window is wide enough for two panes.
    pub two: bool,
    /// One pane's width in CSS pixels, for `panes` panes.
    pub pane_width: f32,
    /// One pane's width in cells, past its padding.
    pub cells: usize,
    /// How many lines of items a pane has: the window less the footer, the
    /// space above, and the breadcrumb's line.
    pub room: usize,
    pub line: f32,
}

impl Geometry {
    pub fn of(width: f32, height: f32, look: &Look, panes: usize) -> Self {
        let line = look.line();
        let pane_width = width / panes.max(1) as f32;
        let cells = ((pane_width - 2.0 * PAD) / look.cell()).floor().max(8.0) as usize;
        let room = ((height - FOOTER - TOP) / line - 1.0).floor().max(1.0) as usize;
        Self {
            two: width >= TWO_PANE_MIN,
            pane_width,
            cells,
            room,
            line,
        }
    }
}

/// What the editor keeps between events.
pub struct State<B: Backend> {
    pub model: RefCell<Model<B>>,
    pub window: Arc<dyn Window>,
    pub host: Rc<dyn Host<B>>,
    /// The wheel's travel not yet spent on a step of the cursor.
    wheel: Cell<f64>,
}

/// What the render reads. Each is a signal, so the page is drawn again when
/// an event changes one.
pub struct Signals {
    /// Goes up with every change to the model.
    pub tick: Signal<u64>,
    /// The window's size in CSS pixels, from winit: dioxus-native has no
    /// `onresize`.
    pub size: Signal<(f32, f32)>,
    pub look: Signal<Look>,
}

impl Clone for Signals {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for Signals {}

/// The editor, as every event source holds it.
pub struct Editor<B: Backend + 'static = FigBackend> {
    pub(crate) st: Rc<State<B>>,
    pub(crate) sig: Signals,
}

impl<B: Backend + 'static> Clone for Editor<B> {
    fn clone(&self) -> Self {
        Self {
            st: self.st.clone(),
            sig: self.sig,
        }
    }
}

impl<B: Backend + 'static> PartialEq for Editor<B> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.st, &other.st)
    }
}

/// The editor, made as hooks of the calling component, in the window the
/// component is in: on the model `model` makes, drawn with the [`Look`]
/// `look` makes, asking `host` for what it cannot do itself. Each is called
/// once, on the first render.
///
/// The model is put in the page projection, its inline budget sized to the
/// window, and opened at the first page that says something — what the
/// terminal app does before its first frame.
///
/// The host puts [`Editor::render`] in its page and hands the editor the
/// window's events: keys through [`crate::pressed`] to [`Editor::key`], and
/// the rest to [`Editor::window_event`].
pub fn use_editor<B: Backend + 'static>(
    model: impl FnOnce() -> Model<B>,
    look: impl FnOnce() -> Look,
    host: impl FnOnce() -> Rc<dyn Host<B>>,
) -> Editor<B> {
    let window = dioxus_native::use_window();
    let size = window.surface_size();
    let scale = window.scale_factor();
    let sig = Signals {
        tick: use_signal(|| 0),
        size: use_signal(|| css_size(size.width, size.height, scale)),
        look: use_signal(look),
    };
    use_hook(|| {
        let st = State {
            model: RefCell::new(model()),
            window: window.clone(),
            host: host(),
            wheel: Cell::new(0.0),
        };
        let ed = Editor {
            st: Rc::new(st),
            sig,
        };
        ed.open();
        ed
    })
}

/// A size in physical pixels, in CSS ones.
fn css_size(width: u32, height: u32, scale: f64) -> (f32, f32) {
    (
        (width as f64 / scale) as f32,
        (height as f64 / scale) as f32,
    )
}

/// Set a signal through a copy of its handle.
fn set<T: 'static>(mut s: Signal<T>, v: T) {
    s.set(v);
}

impl<B: Backend + 'static> Editor<B> {
    /// The model, to read or change. A change is drawn on the next
    /// [`refresh`](Self::refresh).
    pub fn model(&self) -> RefMut<'_, Model<B>> {
        self.st.model.borrow_mut()
    }

    /// Put a different document in the editor — Open, New — and open it as
    /// the first one was.
    pub fn install(&self, model: Model<B>) {
        *self.st.model.borrow_mut() = model;
        self.st.wheel.set(0.0);
        self.open();
    }

    /// Draw with `look` from now on. A new size is a new room, so the inline
    /// budget is fitted to it again.
    pub fn set_look(&self, look: Look) {
        set(self.sig.look, look);
        self.refresh();
    }

    /// The look the editor draws with.
    pub fn look(&self) -> Look {
        self.sig.look.peek().clone()
    }

    /// Whether the editor draws dark: the look's appearance, or the window's
    /// when it follows the system.
    pub fn dark(&self) -> bool {
        match self.sig.look.peek().appearance {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::System => self.st.window.theme() == Some(Theme::Dark),
        }
    }

    /// Where everything goes, for the window as it is now.
    pub fn geometry(&self) -> Geometry {
        let (w, h) = *self.sig.size.peek();
        let look = self.sig.look.peek();
        let two =
            Geometry::of(w, h, &look, 2).two && !self.st.model.borrow().pages_would_degenerate();
        Geometry::of(w, h, &look, if two { 2 } else { 1 })
    }

    /// The first paint of a model: the page projection, the budget for the
    /// window, and the first page that says something.
    fn open(&self) {
        // Asked before the model is borrowed: the geometry reads it too.
        let room = self.geometry().room;
        {
            let mut m = self.st.model.borrow_mut();
            m.set_view(ViewMode::Pages);
            m.fit_to_room(room);
            m.enter_document();
        }
        self.refresh();
    }

    /// Draw the model again, after any change to it. Fits the inline budget
    /// to the room first — cheap, since the model rebuilds only when the
    /// answer moves — so a resize, a zoom and a document that grew past the
    /// window all lay out for what there is.
    pub fn refresh(&self) {
        let room = self.geometry().room;
        {
            let mut m = self.st.model.borrow_mut();
            m.fit_to_room(room);
            self.st.host.repainted(&m);
        }
        let n = *self.sig.tick.peek();
        set(self.sig.tick, n.wrapping_add(1));
    }

    /// A key, from the window ([`crate::pressed`] makes these from winit's)
    /// or replayed by a host's menu. Answers whether it was the editor's, so
    /// a host sharing the window with something else can offer it on.
    pub fn key(&self, key: Key, code: Code, mods: Modifiers) -> bool {
        let _ = code;
        let action = apply_key(&mut self.st.model.borrow_mut(), &key, mods);
        match action {
            Action::Ignored => return false,
            Action::Handled => {}
            Action::Request(r) => {
                self.st.host.request(self, r);
                return true;
            }
            Action::Paste => {
                if let Some(text) = self.st.host.read_clipboard() {
                    insert_text(&mut self.st.model.borrow_mut(), &text);
                }
            }
        }
        self.refresh();
        true
    }

    /// The window's events the editor reads besides its keys: its size, the
    /// theme, and an input method's commit into an open value.
    pub fn window_event(&self, event: &WindowEvent) {
        match event {
            WindowEvent::SurfaceResized(size) => {
                let scale = self.st.window.scale_factor();
                let now = css_size(size.width, size.height, scale);
                if now != *self.sig.size.peek() {
                    set(self.sig.size, now);
                    self.refresh();
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                if insert_text(&mut self.st.model.borrow_mut(), text) {
                    self.refresh();
                }
            }
            // Blitz restyles for the new theme but asks for no frame.
            WindowEvent::ThemeChanged(_) => {
                self.refresh();
                self.st.window.request_redraw();
            }
            _ => {}
        }
    }

    /// A press on item `index` of the pane drawn as `role`.
    ///
    /// On the page the cursor is on, a press stands on the item, and a second
    /// press on the item the cursor stands on opens it — a container as a
    /// page, a value for editing — as Return does. A press on the page one
    /// level out backs out onto the item pressed; one on the preview opens it
    /// onto the item pressed. Ignored while a value is open, so a press cannot
    /// move the cursor out from under a half-typed value.
    pub fn press(&self, role: Role, index: usize) {
        {
            let mut m = self.st.model.borrow_mut();
            if !matches!(m.mode, Mode::Normal) {
                return;
            }
            match role {
                Role::Current if index == m.page_selected() => m.page_enter(),
                Role::Current => m.page_select(index),
                Role::Parent => {
                    m.page_back();
                    m.page_select(index);
                }
                Role::Peek => {
                    m.page_enter();
                    m.page_select(index);
                }
            }
        }
        self.refresh();
    }

    /// A press on choice `index` of the picker's filtered list: that choice,
    /// committed.
    pub fn choose(&self, index: usize) {
        {
            let mut m = self.st.model.borrow_mut();
            let Mode::Choosing { selected, .. } = m.mode else {
                return;
            };
            let n = m.visible_choices().len();
            if index >= n {
                return;
            }
            for _ in selected..index {
                m.choose_next();
            }
            for _ in index..selected {
                m.choose_prev();
            }
            m.choose_commit();
        }
        self.refresh();
    }

    /// A press outside the picker while it is open: called off, as Escape.
    pub fn dismiss(&self) {
        let mut m = self.st.model.borrow_mut();
        match m.mode {
            Mode::Choosing { .. } => m.choose_cancel(),
            _ => return,
        }
        drop(m);
        self.refresh();
    }

    /// The wheel and the trackpad, in CSS pixels, a positive delta toward the
    /// top: a line's travel is a step of the cursor, as the terminal's wheel
    /// is. The list scrolls by following the cursor, so there is one scroll
    /// position and it is always where the cursor is.
    pub fn wheel(&self, dy: f64) {
        if !matches!(self.st.model.borrow().mode, Mode::Normal) {
            return;
        }
        let line = self.sig.look.peek().line() as f64;
        let mut travel = self.st.wheel.get() + dy;
        let mut moved = false;
        while travel.abs() >= line {
            let mut m = self.st.model.borrow_mut();
            if travel > 0.0 {
                m.page_move_up();
                travel -= line;
            } else {
                m.page_move_down();
                travel += line;
            }
            moved = true;
        }
        self.st.wheel.set(travel);
        if moved {
            self.refresh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_too_narrow_for_two_panes_is_one_column() {
        let look = Look::default();
        assert!(!Geometry::of(500.0, 600.0, &look, 2).two);
        let g = Geometry::of(1000.0, 600.0, &look, 2);
        assert!(g.two);
        assert_eq!(g.pane_width, 500.0);
        let one = Geometry::of(1000.0, 600.0, &look, 1);
        assert!(one.cells > 2 * g.cells);
    }

    #[test]
    fn the_room_is_the_window_less_the_chrome_in_whole_lines() {
        let look = Look::default();
        let line = look.line();
        let h = FOOTER + TOP + line * 11.0 + 3.0;
        assert_eq!(Geometry::of(800.0, h, &look, 1).room, 10);
        // A bigger type is fewer lines in the same window.
        let big = Look {
            size: 28.0,
            ..Look::default()
        };
        assert!(Geometry::of(800.0, h, &big, 1).room < 10);
    }
}
