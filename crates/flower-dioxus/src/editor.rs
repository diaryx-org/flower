//! The editor: the model, the host, the page's [`Unit`], and the few signals
//! the page is drawn from — and every way into it: keys, the window's events,
//! presses on rows, chips, switches and menus, and the wheel.
//!
//! An [`Editor`] is a cheap handle — shared state behind an `Rc`, and signals,
//! which are `Copy` — so every event source holds a clone and calls a method
//! on it. The page is drawn again from the model on every change
//! ([`Editor::refresh`] bumps a signal the render reads): a page is a
//! screenful of rows, not a document of thousands, so there is nothing to
//! memoize.

use std::cell::{Cell, RefCell, RefMut};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use dioxus::html::{Code, Key, Modifiers};
use dioxus::prelude::*;
use dioxus_native::winit::dpi::{LogicalPosition, LogicalSize};
use dioxus_native::winit::event::{Ime, MouseScrollDelta, WindowEvent};
use dioxus_native::winit::window::{
    ImeCapabilities, ImeEnableRequest, ImeRequest, ImeRequestData, ImeSurroundingText, Window,
};
use flower_core::{
    Backend, EditSlot, FigBackend, InlineBudget, ItemKind, Mode, Model, NoticeKind, Seg, ViewMode,
};
use unit::{Answer, Dialog, Toast, Tone, Unit, use_unit};

use crate::host::Host;
use crate::ime;
use crate::input::{Action, apply_key, insert_text};
use crate::layout::{self, Laid, Placed, Role, Sizes};

/// Below this many CSS pixels the two panes leave neither one usable, so the
/// editor is one column — the Swift host's `twoPaneMinWidth`.
pub const TWO_PANE_MIN: f64 = 620.0;

/// How long a toast stays: a refusal with no row on screen to be said under.
const TOAST: Duration = Duration::from_secs(4);

/// What the editor draws with: the host's display choices. The colours and
/// the face are the page's [`unit::Theme`], and light or dark is the
/// [`unit::UnitRoot`]'s `appearance`: both the host's, set once for every
/// control in the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// How large everything on the page is drawn: 1 is unit's regular
    /// density, a Mac's sizes.
    pub zoom: f64,
    /// What the document's root is called in the breadcrumb: a file's name,
    /// usually. flower-core has no name for it — the document is bytes the
    /// host opened.
    pub root: String,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            root: "Document".into(),
        }
    }
}

/// The part of the window the editor has: the host's own chrome above it (a
/// toolbar) and below it, in CSS pixels. The editor fills the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub top: f64,
    pub bottom: f64,
}

/// Where everything goes in the editor's box, for a look: how many panes
/// there is room for, how wide and tall each is, and how many rows each has
/// room for. Arithmetic on the layout's sizes rather than a measurement,
/// because the model's inline budget has to be sized *before* the frame it
/// applies to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// Whether the box is wide enough for two panes.
    pub two: bool,
    /// The editor's box.
    pub width: f64,
    pub height: f64,
    /// One pane's width, for `panes` panes, and every pane's height: the box
    /// less the breadcrumb's bar.
    pub pane_width: f64,
    pub pane_height: f64,
    /// How many one-line rows a pane has room for.
    pub room: usize,
    pub sizes: Sizes,
}

impl Geometry {
    pub fn of(width: f64, height: f64, look: &Look, panes: usize) -> Self {
        let sizes = Sizes::at(look.zoom);
        let pane_width = width / panes.max(1) as f64;
        let pane_height = (height - sizes.bar).max(0.0);
        let room = ((pane_height - 2.0 * sizes.pane_pad) / (sizes.row + sizes.divider))
            .floor()
            .max(1.0) as usize;
        Self {
            two: width >= TWO_PANE_MIN,
            width,
            height,
            pane_width,
            pane_height,
            room,
            sizes,
        }
    }

    /// The width of the column of sections in a pane: the pane less its
    /// padding, up to the column's widest.
    pub fn column(&self) -> f64 {
        (self.pane_width - 2.0 * self.sizes.pane_pad).min(self.sizes.column_max)
    }
}

/// Where one of the drawn panes is scrolled to, and over which page: a pane
/// that shows another page starts at its top.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PaneScroll {
    pub focus: Option<Vec<Seg>>,
    pub offset: f64,
    /// The content's height when it was last laid out.
    pub content: f64,
}

/// What the editor keeps between events.
pub struct State<B: Backend> {
    pub model: RefCell<Model<B>>,
    pub window: Arc<dyn Window>,
    pub host: Rc<dyn Host<B>>,
    pub unit: Unit,
    /// The input method's open composition, drawn after the open field's
    /// text and not in it until it commits (`ime.rs`).
    pub marked: RefCell<String>,
    /// Each drawn pane's scroll, left to right.
    pub(crate) scroll: RefCell<[PaneScroll; 2]>,
    /// The inline budget the host chose, or `None` to fit it to the room.
    budget: Cell<Option<InlineBudget>>,
    /// The pages whose "Advanced" fold is open, by their focus. Reset by
    /// nothing: a fold opened stays open for the session, as a disclosure
    /// in a settings window does.
    pub(crate) advanced: RefCell<HashSet<Vec<Seg>>>,
    /// The last notice said as a toast, so it is said once.
    toasted: Cell<u64>,
    /// Where the cursor was when the panes were last scrolled to it.
    revealed: RefCell<Option<(Vec<Seg>, usize)>>,
    /// Where the pointer is across the window, in CSS pixels: which pane a
    /// wheel scrolls.
    pointer: Cell<f64>,
}

/// What the render reads. Each is a signal, so the page is drawn again when
/// an event changes one.
pub struct Signals {
    /// Goes up with every change to the model.
    pub tick: Signal<u64>,
    /// The window's size in CSS pixels, from winit: dioxus-native has no
    /// `onresize`.
    pub size: Signal<(f64, f64)>,
    pub look: Signal<Look>,
    pub bounds: Signal<Bounds>,
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
/// The component must be under a [`unit::UnitRoot`] with `keys: Keys::Host`:
/// the editor's controls are unit's, its picker and prompts are drawn in the
/// root's overlay, and the host hands the window's keys to the editor, which
/// offers each to the page's controls first.
///
/// The model is put in the page projection, its inline budget sized to the
/// window, and opened at the first page that says something — what the
/// terminal app does before its first frame.
pub fn use_editor<B: Backend + 'static>(
    model: impl FnOnce() -> Model<B>,
    look: impl FnOnce() -> Look,
    host: impl FnOnce() -> Rc<dyn Host<B>>,
) -> Editor<B> {
    let window = dioxus_native::use_window();
    let unit = use_unit();
    let size = window.surface_size();
    let scale = window.scale_factor();
    let sig = Signals {
        tick: use_signal(|| 0),
        size: use_signal(|| css_size(size.width, size.height, scale)),
        look: use_signal(look),
        bounds: use_signal(Bounds::default),
    };
    use_hook(|| {
        let st = State {
            model: RefCell::new(model()),
            window: window.clone(),
            host: host(),
            unit,
            marked: RefCell::new(String::new()),
            scroll: RefCell::default(),
            budget: Cell::new(None),
            advanced: RefCell::default(),
            toasted: Cell::new(0),
            revealed: RefCell::new(None),
            pointer: Cell::new(0.0),
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
fn css_size(width: u32, height: u32, scale: f64) -> (f64, f64) {
    (width as f64 / scale, height as f64 / scale)
}

/// Set a signal through a copy of its handle.
fn set<T: 'static>(mut s: Signal<T>, v: T) {
    s.set(v);
}

/// One pane, laid out: what it shows, and where its rows sit.
pub(crate) struct PaneGeometry {
    pub role: Role,
    pub focus: Vec<Seg>,
    pub selected: Option<usize>,
    pub placed: Placed,
}

impl<B: Backend + 'static> Editor<B> {
    /// The model, to read or change. A change is drawn on the next
    /// [`refresh`](Self::refresh).
    pub fn model(&self) -> RefMut<'_, Model<B>> {
        self.st.model.borrow_mut()
    }

    /// A number that changes with every repaint, read: a component of the
    /// host's drawn from the model — a toolbar's enabled buttons — calls it to
    /// be drawn again with the editor.
    pub fn changed(&self) -> u64 {
        *self.sig.tick.read()
    }

    /// The page's [`Unit`]: its overlay, which a host opens its own dialogs
    /// and toasts in.
    pub fn unit(&self) -> Unit {
        self.st.unit
    }

    /// Put a different document in the editor — Open, New — and open it as
    /// the first one was.
    pub fn install(&self, model: Model<B>) {
        *self.st.model.borrow_mut() = model;
        *self.st.scroll.borrow_mut() = Default::default();
        self.st.advanced.borrow_mut().clear();
        *self.st.revealed.borrow_mut() = None;
        self.open();
    }

    /// Draw with `look` from now on. A new size is a new room, so the inline
    /// budget is fitted to it again.
    pub fn set_look(&self, look: Look) {
        if *self.sig.look.peek() != look {
            set(self.sig.look, look);
            self.refresh();
        }
    }

    /// The look the editor draws with.
    pub fn look(&self) -> Look {
        self.sig.look.peek().clone()
    }

    /// The host's chrome above and below the editor.
    pub fn set_bounds(&self, bounds: Bounds) {
        if *self.sig.bounds.peek() != bounds {
            set(self.sig.bounds, bounds);
            self.refresh();
        }
    }

    /// How much of the document inlines onto one page before it drills: a
    /// budget the host chose, or `None` to fit the budget to the room, which
    /// is the default and what a resize follows.
    pub fn set_budget(&self, budget: Option<InlineBudget>) {
        self.st.budget.set(budget);
        match budget {
            Some(b) => self.st.model.borrow_mut().set_inline_budget(b),
            None => *self.st.revealed.borrow_mut() = None,
        }
        self.refresh();
    }

    /// The budget the host chose, if it chose one.
    pub fn budget(&self) -> Option<InlineBudget> {
        self.st.budget.get()
    }

    /// Where everything goes, for the window as it is now.
    pub fn geometry(&self) -> Geometry {
        let (w, h) = *self.sig.size.peek();
        let b = *self.sig.bounds.peek();
        let h = (h - b.top - b.bottom).max(0.0);
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
            match self.st.budget.get() {
                Some(b) => m.set_inline_budget(b),
                None => m.fit_to_room(room),
            }
            m.enter_document();
        }
        self.refresh();
    }

    /// Draw the model again, after any change to it. Fits the inline budget
    /// to the room first — cheap, since the model rebuilds only when the
    /// answer moves — so a resize, a zoom and a document that grew past the
    /// window all lay out for what there is; scrolls each pane to keep the
    /// cursor in view; and says a notice with no row to be said under.
    pub fn refresh(&self) {
        if self.st.budget.get().is_none() {
            let room = self.geometry().room;
            self.st.model.borrow_mut().fit_to_room(room);
        }
        self.follow_cursor();
        self.toast_notice();
        {
            let m = self.st.model.borrow();
            self.st.host.repainted(&m);
        }
        let n = *self.sig.tick.peek();
        set(self.sig.tick, n.wrapping_add(1));
        self.sync_ime();
    }

    // ── laying the panes out ────────────────────────────────────────────────

    /// How many lines a row has under its title — a refusal said under it, a
    /// host's finding, the comment written above it — as `view.rs` draws it.
    pub(crate) fn sublines(&self, model: &Model<B>, item: &flower_core::PageItem) -> usize {
        let said = notice_under(model, &item.path).is_some();
        let found = item.annotation.is_some();
        let note = item.leading_comment.is_some()
            || matches!(&model.mode, Mode::Editing { path, slot: EditSlot::LeadingComment, .. }
                if *path == item.path);
        usize::from(said || found) + usize::from(note)
    }

    /// The panes as drawn: each one's page laid out and placed.
    pub(crate) fn pane_geometry(&self) -> Vec<Option<PaneGeometry>> {
        let g = self.geometry();
        let m = self.st.model.borrow();
        let advanced = self.st.advanced.borrow();
        layout::panes(&m, g.two)
            .into_iter()
            .map(|p| {
                let s = p?;
                let laid = layout::lay_out(&s.page, g.column(), &g.sizes);
                let open = advanced.contains(&s.page.focus) || fold_held_open(&m, &laid, &s);
                let adds = s.role == Role::Current && adds_here(&m, &s.page.focus);
                let sub = |i: &flower_core::PageItem| self.sublines(&m, i);
                let placed =
                    layout::place(&laid, s.role != Role::Current, adds, open, &sub, &g.sizes);
                Some(PaneGeometry {
                    role: s.role,
                    focus: s.page.focus.clone(),
                    selected: s.selected,
                    placed,
                })
            })
            .collect()
    }

    /// Scroll each pane just far enough to show the row it marks, when the
    /// cursor has moved since the last time; and a pane that shows another
    /// page from its top.
    fn follow_cursor(&self) {
        let g = self.geometry();
        let panes = self.pane_geometry();
        let now = {
            let m = self.st.model.borrow();
            (m.focus().to_vec(), m.page_selected())
        };
        let moved = self.st.revealed.borrow().as_ref() != Some(&now);
        let mut scroll = self.st.scroll.borrow_mut();
        for (slot, pane) in panes.iter().enumerate().take(2) {
            let s = &mut scroll[slot];
            let Some(p) = pane else {
                *s = PaneScroll::default();
                continue;
            };
            let fresh = s.focus.as_ref() != Some(&p.focus);
            if fresh {
                *s = PaneScroll {
                    focus: Some(p.focus.clone()),
                    ..PaneScroll::default()
                };
            }
            s.content = p.placed.height;
            if (fresh || moved)
                && let Some(sel) = p.selected
                && let Some((y, h)) = p.placed.of(sel)
            {
                let margin = g.sizes.pane_pad;
                if y - margin < s.offset {
                    s.offset = y - margin;
                } else if y + h + margin > s.offset + g.pane_height {
                    s.offset = y + h + margin - g.pane_height;
                }
            }
            s.offset = s.offset.clamp(0.0, (s.content - g.pane_height).max(0.0));
        }
        *self.st.revealed.borrow_mut() = Some(now);
    }

    /// A refusal or a warning about no row on screen: a toast, since there is
    /// nowhere better to say it. One about a row is said under the row.
    fn toast_notice(&self) {
        let (seq, toast) = {
            let m = self.st.model.borrow();
            let Some(n) = &m.notice else { return };
            if n.seq == self.st.toasted.get() || m.status.is_empty() {
                return;
            }
            let tone = match n.kind {
                NoticeKind::Done => return,
                NoticeKind::Warning => Tone::Warning,
                NoticeKind::Rejected => Tone::Danger,
            };
            let shown = n.path.as_ref().is_some_and(|p| {
                m.page().position_of(p).is_some()
                    || (!m.focus().is_empty() && m.parent_page().position_of(p).is_some())
            });
            if shown {
                (n.seq, None)
            } else {
                (
                    n.seq,
                    Some(Toast::new(n.message.clone()).tone(tone).lasts(TOAST)),
                )
            }
        };
        self.st.toasted.set(seq);
        if let Some(t) = toast {
            self.st.unit.toast(t);
        }
    }

    // ── the input method ────────────────────────────────────────────────────

    /// The input method on while a value is open and off otherwise, told
    /// where the caret is, so its candidate window and the accent menu open
    /// beside it, and what text is before the caret, so press-and-hold can
    /// see the letter it offers accents for (`ime.rs`).
    fn sync_ime(&self) {
        let window = &self.st.window;
        let on = window.ime_capabilities().is_some();
        let around = ime::surrounding(&self.st.model.borrow());
        let Some((text, caret)) = around else {
            self.st.marked.borrow_mut().clear();
            if on {
                let _ = window.request_ime_update(ImeRequest::Disable);
            }
            return;
        };
        let (x, y, h) = self.caret_box();
        let mut data = ImeRequestData::default().with_cursor_area(
            LogicalPosition::new(x, y).into(),
            LogicalSize::new(2.0, h).into(),
        );
        if let Ok(text) = ImeSurroundingText::new(text, caret, caret) {
            data = data.with_surrounding_text(text);
        }
        let request = if on {
            Some(ImeRequest::Update(data))
        } else {
            ImeEnableRequest::new(
                ImeCapabilities::new()
                    .with_cursor_area()
                    .with_surrounding_text(),
                data,
            )
            .map(ImeRequest::Enable)
        };
        if let Some(request) = request {
            let _ = window.request_ime_update(request);
        }
    }

    /// Where the open field's caret is, in the window's CSS pixels: at the
    /// right of the row being edited, where a value's field ends — its text
    /// is right-aligned, so the caret, at its end, is always there.
    fn caret_box(&self) -> (f64, f64, f64) {
        let g = self.geometry();
        let top = self.sig.bounds.peek().top + g.sizes.bar;
        let path = match &self.st.model.borrow().mode {
            Mode::Editing { path, .. } => path.clone(),
            _ => Vec::new(),
        };
        let panes = self.pane_geometry();
        let scroll = self.st.scroll.borrow();
        for (slot, p) in panes.iter().enumerate() {
            let Some(p) = p else { continue };
            if p.role != Role::Current {
                continue;
            }
            let m = self.st.model.borrow();
            let index = m.page().position_of(&path).unwrap_or(m.page_selected());
            let right = g.pane_width * (slot + 1) as f64
                - (g.pane_width - g.column()) / 2.0
                - g.sizes.pane_pad;
            if let Some((y, h)) = p.placed.of(index) {
                return (right, top + y - scroll[slot].offset, h);
            }
            return (right, top, g.sizes.row);
        }
        (g.sizes.pane_pad, top, g.sizes.row)
    }

    /// The input method: a composition drawn, a commit typed into what is
    /// open, and text before the caret taken out for what replaces it.
    fn ime(&self, event: &Ime) {
        match event {
            Ime::Preedit(text, _) => *self.st.marked.borrow_mut() = text.clone(),
            Ime::Commit(text) => {
                self.st.marked.borrow_mut().clear();
                if !self.st.unit.text(text) {
                    insert_text(&mut self.st.model.borrow_mut(), text);
                }
            }
            Ime::DeleteSurrounding { before_bytes, .. } => {
                ime::delete_before(&mut self.st.model.borrow_mut(), *before_bytes);
            }
            Ime::Disabled => self.st.marked.borrow_mut().clear(),
            _ => return,
        }
        self.refresh();
    }

    // ── keys and the window ─────────────────────────────────────────────────

    /// A key, from the window ([`crate::pressed`] makes these from winit's)
    /// or replayed by a host's menu. Offered to the page's controls first — an
    /// open menu, a prompt, the picker's Escape — then the editor's table.
    /// Answers whether it was taken, so a host sharing the window with
    /// something else can offer it on.
    pub fn key(&self, key: Key, code: Code, mods: Modifiers) -> bool {
        let _ = code;
        if self.st.unit.key(&key, mods) {
            self.refresh();
            return true;
        }
        let action = apply_key(&mut self.st.model.borrow_mut(), &key, mods);
        match action {
            Action::Ignored => return false,
            Action::Handled => self.st.unit.blur(),
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
    /// theme, and the input method while a value is open.
    pub fn window_event(&self, event: &WindowEvent) {
        match event {
            WindowEvent::SurfaceResized(size) => {
                let scale = self.st.window.scale_factor();
                let now = css_size(size.width, size.height, scale);
                if now != *self.sig.size.peek() {
                    set(self.sig.size, now);
                    self.st.unit.set_viewport(now.0, now.1);
                    self.refresh();
                }
            }
            WindowEvent::Ime(event) => self.ime(event),
            WindowEvent::PointerMoved { position, .. } => {
                self.st
                    .pointer
                    .set(position.x / self.st.window.scale_factor());
            }
            // From winit rather than the page's `onwheel`, which Blitz does
            // not send to an element that does not scroll by itself.
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y as f64 * unit::scroll::LINE,
                    MouseScrollDelta::PixelDelta(p) => p.y / self.st.window.scale_factor(),
                    _ => return,
                };
                let g = self.geometry();
                let slot = (self.st.pointer.get() / g.pane_width.max(1.0)) as usize;
                self.wheel(slot.min(1), dy);
            }
            // Blitz restyles for the new theme but asks for no frame.
            WindowEvent::ThemeChanged(_) => {
                self.refresh();
                self.st.window.request_redraw();
            }
            _ => {}
        }
    }

    // ── presses ─────────────────────────────────────────────────────────────

    /// Change the model, then draw it. A value left open by a press elsewhere
    /// is committed first, as a text field that loses focus is; a picker left
    /// open is called off.
    fn act(&self, f: impl FnOnce(&mut Model<B>)) {
        // The keys are the editor's again: a switch or a menu pressed to get
        // here would otherwise keep them, and take Return.
        self.st.unit.blur();
        {
            let mut m = self.st.model.borrow_mut();
            match m.mode {
                Mode::Editing { .. } => m.edit_commit(),
                Mode::Choosing { .. } => m.choose_cancel(),
                Mode::Normal => {}
            }
            f(&mut m);
        }
        self.refresh();
    }

    /// A press on item `index` of the pane drawn as `role`: the row the
    /// cursor goes to, and what a settings row does when it is pressed — a
    /// container opens as a page, a value opens for typing (or its picker,
    /// where it has a vocabulary).
    ///
    /// A press on the page one level out backs out onto the item pressed and
    /// opens it there, so a sibling is a press away; one on the preview opens
    /// the page it previews, onto the item pressed.
    pub fn press(&self, role: Role, index: usize) {
        self.act(|m| {
            match role {
                Role::Current => {}
                Role::Parent => m.page_back(),
                Role::Peek => m.page_enter(),
            }
            m.page_select(index);
            if role == Role::Peek {
                return;
            }
            let Some(item) = m.page_item() else { return };
            match item.kind {
                ItemKind::Drill { .. } => m.page_enter(),
                ItemKind::Scalar if item.vkind != flower_core::VKind::Bool => m.page_enter(),
                _ => {}
            }
        });
    }

    /// The cursor onto item `index` of the current page, doing nothing else:
    /// a press on a row's control, which does its own thing.
    pub fn select(&self, index: usize) {
        self.act(|m| m.page_select(index));
    }

    /// Open the page at `focus`: a breadcrumb's step, `[]` the document root.
    pub fn open_page(&self, focus: Vec<Seg>) {
        self.act(|m| {
            if focus.is_empty() {
                while !m.focus().is_empty() {
                    m.page_back();
                }
                return;
            }
            m.focus_on(&focus);
            if m.page_item().is_some_and(|i| i.is_drill()) {
                m.page_enter();
            }
        });
    }

    /// Back to the page that listed this one.
    pub fn back(&self) {
        self.act(|m| m.page_back());
    }

    /// A switch: the boolean at `path` set to `on`.
    pub fn set_bool(&self, path: Vec<Seg>, on: bool) {
        self.act(|m| {
            m.focus_on(&path);
            m.set_scalar_text(&path, if on { "true" } else { "false" });
        });
    }

    /// A vocabulary's menu: the field at `path` set to its `n`th choice.
    pub fn set_choice(&self, path: Vec<Seg>, n: usize) {
        self.act(|m| {
            m.focus_on(&path);
            if let Some(c) = m.choices_at(&path).and_then(|c| c.into_iter().nth(n)) {
                m.set_value_at(&path, c.value);
            }
        });
    }

    /// The node at `path` opened for typing `slot`: its value, or one of its
    /// comments.
    pub fn edit(&self, path: Vec<Seg>, slot: EditSlot) {
        self.act(|m| {
            m.focus_on(&path);
            match slot {
                EditSlot::Value => m.begin_edit(),
                EditSlot::LeadingComment => m.begin_edit_leading_comment(),
                EditSlot::TrailingComment => m.begin_edit_trailing_comment(),
                EditSlot::NewItem => m.begin_append(&path),
            }
        });
    }

    /// The node at `path`'s picker, where it has a vocabulary.
    pub fn choose(&self, path: Vec<Seg>) {
        self.act(|m| {
            m.focus_on(&path);
            m.begin_choose();
        });
    }

    /// Something added to the list at `path`: from its vocabulary where it
    /// has one, typed where it does not.
    pub fn add_item(&self, path: Vec<Seg>) {
        self.act(|m| m.begin_choose_append(&path));
    }

    /// The node at `path` deleted.
    pub fn delete(&self, path: Vec<Seg>) {
        self.act(|m| {
            m.focus_on(&path);
            m.delete_selected();
        });
    }

    /// The node at `path` moved one place up (`by` < 0) or down among its
    /// siblings.
    pub fn move_by(&self, path: Vec<Seg>, by: isize) {
        self.act(|m| {
            m.focus_on(&path);
            if by < 0 {
                m.move_selected_up();
            } else {
                m.move_selected_down();
            }
        });
    }

    /// Undo, and redo.
    pub fn undo(&self) {
        self.act(|m| {
            m.undo();
        });
    }

    pub fn redo(&self) {
        self.act(|m| {
            m.redo();
        });
    }

    /// The fold on the page at `focus` opened or shut.
    pub fn toggle_advanced(&self, focus: Vec<Seg>) {
        {
            let mut a = self.st.advanced.borrow_mut();
            if !a.remove(&focus) {
                a.insert(focus);
            }
        }
        self.refresh();
    }

    /// A press on choice `index` of the picker's filtered list: that choice,
    /// committed.
    pub fn choose_at(&self, index: usize) {
        {
            let mut m = self.st.model.borrow_mut();
            let Mode::Choosing { selected, .. } = m.mode else {
                return;
            };
            if index >= m.visible_choices().len() {
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

    /// The picker called off: Escape, or a press outside it.
    pub fn dismiss(&self) {
        let mut m = self.st.model.borrow_mut();
        match m.mode {
            Mode::Choosing { .. } => m.choose_cancel(),
            Mode::Editing { .. } => m.edit_cancel(),
            Mode::Normal => return,
        }
        drop(m);
        self.refresh();
    }

    /// The wheel and the trackpad over drawn pane `slot`, in CSS pixels, a
    /// positive delta toward the top. The pane scrolls; the cursor stays
    /// where it is, as a settings window's does.
    pub fn wheel(&self, slot: usize, dy: f64) {
        let g = self.geometry();
        let moved = {
            let mut scroll = self.st.scroll.borrow_mut();
            let Some(s) = scroll.get_mut(slot) else {
                return;
            };
            let was = s.offset;
            s.offset = (s.offset - dy).clamp(0.0, (s.content - g.pane_height).max(0.0));
            s.offset != was
        };
        if moved {
            let n = *self.sig.tick.peek();
            set(self.sig.tick, n.wrapping_add(1));
        }
    }

    // ── prompts ─────────────────────────────────────────────────────────────

    /// Rename the entry at `path`: a prompt for its new key.
    pub fn rename(&self, path: Vec<Seg>) {
        let Some(Seg::Key(old)) = path.last().cloned() else {
            return;
        };
        let ed = self.clone();
        self.st.unit.open_dialog(
            Dialog::new(format!("Rename “{old}”"))
                .field(old.clone(), "key")
                .buttons("Rename", "Cancel"),
            move |answer| {
                if let Answer::Ok(key) = answer {
                    let key = key.trim().to_string();
                    if !key.is_empty() && key != old {
                        ed.act(|m| m.rename_key(&path, &key));
                    }
                }
            },
        );
    }

    /// Add a field to the mapping at `path`: a prompt for its key, then its
    /// value open for typing. `key` skips the prompt — a field the schema
    /// declares, offered by name.
    pub fn add_field(&self, path: Vec<Seg>, key: Option<String>) {
        let ed = self.clone();
        let add = move |key: String| {
            ed.act(|m| {
                m.insert_key_text(&path, &key, "");
                let mut at = path.clone();
                at.push(Seg::Key(key));
                if m.value_at(&at).is_some() {
                    m.focus_on(&at);
                    m.begin_edit();
                }
            });
        };
        if let Some(k) = key {
            add(k);
            return;
        }
        self.st.unit.open_dialog(
            Dialog::new("Add a Field")
                .message("What is the new field’s key?")
                .field("", "key")
                .buttons("Add", "Cancel"),
            move |answer| {
                if let Answer::Ok(key) = answer {
                    let key = key.trim().to_string();
                    if !key.is_empty() {
                        add(key);
                    }
                }
            },
        );
    }
}

/// The notice said under the row at `path`, if it is one of the kinds a
/// window says at all: a refusal, or a warning.
pub(crate) fn notice_under<'m, B: Backend>(
    model: &'m Model<B>,
    path: &[Seg],
) -> Option<&'m flower_core::Notice> {
    let n = model.notice.as_ref()?;
    (n.kind != NoticeKind::Done && n.path.as_deref() == Some(path) && !model.status.is_empty())
        .then_some(n)
}

/// Whether the page at `focus` offers to add to its own container: a mapping
/// takes a field and a list an item.
pub(crate) fn adds_here<B: Backend>(model: &Model<B>, focus: &[Seg]) -> bool {
    matches!(
        model.value_at(focus),
        Some(fig::Value::Map(_) | fig::Value::Seq(_))
    )
}

/// Whether the fold must show though nobody opened it: it holds the row being
/// typed in, or — on a pane the cursor is not on — the row marked as the way
/// back. A fold that could hide either would be a place where state goes to
/// get lost.
pub(crate) fn fold_held_open<B: Backend>(
    model: &Model<B>,
    laid: &Laid<'_>,
    shown: &layout::Shown<'_>,
) -> bool {
    let editing = match &model.mode {
        Mode::Editing { path, .. } => Some(path.as_slice()),
        _ => None,
    };
    laid.advanced.iter().flat_map(|s| &s.entries).any(|e| {
        Some(e.item().path.as_slice()) == editing
            || (shown.role != Role::Current && shown.selected.is_some_and(|i| e.holds(i)))
    })
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
        assert!(one.column() > g.column());
        // A column never grows past its widest.
        assert_eq!(
            Geometry::of(3000.0, 600.0, &look, 1).column(),
            Sizes::BASE.column_max
        );
    }

    #[test]
    fn the_room_is_the_pane_less_its_padding_in_whole_rows() {
        let look = Look::default();
        let z = Sizes::BASE;
        let h = z.bar + 2.0 * z.pane_pad + (z.row + z.divider) * 10.0 + 3.0;
        assert_eq!(Geometry::of(800.0, h, &look, 1).room, 10);
        // A bigger zoom is fewer rows in the same window.
        let big = Look {
            zoom: 2.0,
            ..Look::default()
        };
        assert!(Geometry::of(800.0, h, &big, 1).room < 10);
    }
}
