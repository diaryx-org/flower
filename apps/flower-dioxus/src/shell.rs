//! The app around the editor: the [`Shell`], which is the editor's [`Host`].
//!
//! The editor asks for what it does not own; this answers as a document
//! window does. Save goes to the document's file, or to a Save As sheet when
//! it has none; the zoom scales the editor's [`Look`]; the clipboard is the
//! system's; and the window's title is the file's name, "— Edited" while it
//! has unsaved changes. ⌘O is the one key the editor leaves alone that the
//! app takes.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use dioxus::html::{Code, Key, Modifiers};
use dioxus_native::winit::window::Window;
use flower_core::{FigBackend, Model};
use flower_dioxus::{Editor, Host, Look, Request, ZoomCommand};

use crate::file::{self, Unsaved};

/// The zoom's steps and its bounds, as a multiple of the look's size.
const ZOOM_STEP: f32 = 1.1;
const ZOOM_MIN: f32 = 0.5;
const ZOOM_MAX: f32 = 3.0;

/// The app's half of the window.
pub struct Shell {
    window: Arc<dyn Window>,
    /// The document's file; `None` while it is untitled.
    path: SharedPath,
    zoom: Cell<f32>,
    /// The window title last set, so it is only set when it changes.
    title: RefCell<String>,
}

impl Shell {
    pub fn new(window: Arc<dyn Window>, path: Option<PathBuf>) -> Self {
        Self {
            window,
            path: Rc::new(RefCell::new(path)),
            zoom: Cell::new(1.0),
            title: RefCell::default(),
        }
    }

    /// A key, from the window or replayed by a menu item: the editor's
    /// first, and what it does not take, the shell's (⌘O).
    pub fn chord(&self, ed: &Editor, key: Key, code: Code, mods: Modifiers) {
        if ed.key(key.clone(), code, mods) {
            return;
        }
        if mods == Modifiers::META
            && let Key::Character(c) = &key
            && c.eq_ignore_ascii_case("o")
        {
            self.open(ed);
        }
    }

    /// ⌘O: settle the document's unsaved changes, ask for a file, and put
    /// it in the editor.
    fn open(&self, ed: &Editor) {
        let (ed, window, path) = (ed.clone(), self.window.clone(), self.path.clone());
        dioxus::prelude::spawn(async move {
            if !settle_unsaved(&ed, &window, &path).await {
                return;
            }
            let Some(chosen) = file::pick_open(&window).await else {
                return;
            };
            match file::read(&chosen) {
                Ok(model) => {
                    *path.borrow_mut() = Some(chosen);
                    name_root(&ed, &path);
                    ed.install(model);
                }
                Err(e) => {
                    ed.model().set_status(e);
                    ed.refresh();
                }
            }
        });
    }

    fn zoom(&self, ed: &Editor, z: ZoomCommand) {
        let now = match z {
            ZoomCommand::In => self.zoom.get() * ZOOM_STEP,
            ZoomCommand::Out => self.zoom.get() / ZOOM_STEP,
            ZoomCommand::Reset => 1.0,
        }
        .clamp(ZOOM_MIN, ZOOM_MAX);
        self.zoom.set(now);
        ed.set_look(Look {
            zoom: now as f64,
            ..ed.look()
        });
    }
}

impl Host for Shell {
    fn request(&self, ed: &Editor, request: Request) {
        match request {
            Request::Save => {
                let path = self.path.borrow().clone();
                match path {
                    Some(p) => {
                        file::write(&mut ed.model(), &p);
                        ed.refresh();
                    }
                    // Untitled: only a sheet can say where.
                    None => save_as(ed, &self.window, &self.path),
                }
            }
            Request::SaveAs => save_as(ed, &self.window, &self.path),
            Request::Zoom(z) => self.zoom(ed, z),
        }
    }

    fn read_clipboard(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    /// The file's name, and "Edited" while it has unsaved changes.
    fn repainted(&self, model: &Model<FigBackend>) {
        let name = file::name_of(self.path.borrow().as_deref());
        let now = match model.dirty {
            true => format!("{name} — Edited"),
            false => name,
        };
        if *self.title.borrow() != now {
            self.window.set_title(&now);
            *self.title.borrow_mut() = now;
        }
    }
}

/// The document's file, shared with the tasks the dialogs run in: a task
/// cannot borrow the `Shell` it was spawned from.
type SharedPath = Rc<RefCell<Option<PathBuf>>>;

/// Save As, as a task: a sheet on the window.
fn save_as(ed: &Editor, window: &Arc<dyn Window>, path: &SharedPath) {
    let (ed, window, path) = (ed.clone(), window.clone(), path.clone());
    dioxus::prelude::spawn(async move {
        save_as_now(&ed, &window, &path).await;
    });
}

/// Name a file for the document and write it there; answers whether it was
/// written.
async fn save_as_now(ed: &Editor, window: &Arc<dyn Window>, path: &SharedPath) -> bool {
    let name = file::name_of(path.borrow().as_deref());
    let suggested = if path.borrow().is_none() {
        "Untitled.toml".into()
    } else {
        name
    };
    let Some(to) = file::pick_save(window, &suggested).await else {
        return false;
    };
    let ok = file::write(&mut ed.model(), &to);
    if ok {
        *path.borrow_mut() = Some(to);
        name_root(ed, path);
    }
    ed.refresh();
    ok
}

/// The breadcrumb's root is the file's name, as the window's title is.
fn name_root(ed: &Editor, path: &SharedPath) {
    let root = file::name_of(path.borrow().as_deref());
    ed.set_look(Look { root, ..ed.look() });
}

/// Before the document is replaced: save it, drop its changes, or stay —
/// answers whether to go ahead.
async fn settle_unsaved(ed: &Editor, window: &Arc<dyn Window>, path: &SharedPath) -> bool {
    if !ed.model().dirty {
        return true;
    }
    let name = file::name_of(path.borrow().as_deref());
    match file::ask_unsaved(window, &name).await {
        Unsaved::Discard => true,
        Unsaved::Cancel => false,
        Unsaved::Save => {
            let to = path.borrow().clone();
            match to {
                Some(p) => file::write(&mut ed.model(), &p),
                None => save_as_now(ed, window, path).await,
            }
        }
    }
}
