//! A data file's window: JSON, YAML, TOML, ZON or figl, read as a tree and
//! edited a value at a time by flower-data-editor ([`flower_data_editor`]),
//! where a document's window has flower-doc-editor's caret.
//!
//! The file is the platform document's, as a document's is: the Mac's
//! `NSDocument` reads and writes it and asks the page only for its text;
//! elsewhere the app keeps the file itself. What differs is what the text is
//! read into — a [`Model`] over fig, which holds only text that parses — so
//! a file that does not parse is refused when it is opened ([`model`]), and
//! one changed under an open window into something that does not parse is
//! not taken.
//!
//! The page is the editor's own bar and panes under unit's root, with the
//! [`Toolbar`] at the end of the bar; the [`Shell`] is the editor's
//! [`Host`], answering Save and the zoom through the platform.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dioxus::prelude::*;
use flower_data::{FigBackend, InlineBudget, Mode, Model};
use flower_data_editor::{Editor, FlowerPages, Host, Look, Request, ZoomCommand, use_editor};
use unit::icon::paths;
use unit::{Button, Keys, Segmented, UnitRoot, Variant, use_unit};

use crate::app;
use crate::page::{KeepUnit, Made, unit_appearance};
use crate::platform::{self, DocumentRef};

/// The extensions a data file has, the first of each kind the one Save As
/// suggests: what flower-data's [`detect`](flower_data::detect) reads. For
/// the dialogs the app puts up itself: the Mac's are AppKit's, from the
/// types `Info.plist` declares.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub const TYPES: [(&str, &[&str]); 5] = [
    ("JSON", &["json", "jsonc", "json5"]),
    ("YAML", &["yaml", "yml"]),
    ("TOML", &["toml"]),
    ("ZON", &["zon"]),
    ("fig", &["figl", "fig"]),
];

/// Every extension a data file has.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn extensions() -> Vec<&'static str> {
    TYPES.iter().flat_map(|(_, e)| e.iter().copied()).collect()
}

/// `source` as a model, read in the format `extension` names; or why not,
/// in a line.
pub fn model(source: &str, extension: &str) -> Result<Model<FigBackend>, String> {
    let format = flower_data::format::from_extension(extension)
        .ok_or_else(|| format!("“.{extension}” is not a format Flower reads as data"))?;
    let backend = FigBackend::open(source.as_bytes(), format).map_err(|e| e.to_string())?;
    Model::new(backend).map_err(|e| format!("{e:#}"))
}

/// What a data file's page is made with.
#[derive(Clone)]
pub struct DataInit {
    /// The file's model, until the page takes it.
    model: Rc<RefCell<Option<Model<FigBackend>>>>,
    pub shell: Rc<Shell>,
    pub made: Made,
    /// The editor, once the page has made it.
    pub editor: Rc<RefCell<Option<Editor>>>,
}

impl DataInit {
    pub fn new(model: Model<FigBackend>, shell: Shell) -> Self {
        Self {
            model: Rc::new(RefCell::new(Some(model))),
            shell: Rc::new(shell),
            made: Made::default(),
            editor: Rc::default(),
        }
    }
}

/// The editor's [`Host`] in a data file's window.
pub struct Shell {
    pub document: DocumentRef,
    /// The window's zoom.
    zoom: Cell<f64>,
    /// Set by every repaint: the page has an edit, or a save, to tell the
    /// platform of.
    pub stale: Cell<bool>,
}

const ZOOM_STEP: f64 = 1.1;
const ZOOM_MIN: f64 = 0.6;
const ZOOM_MAX: f64 = 2.5;

impl Shell {
    pub fn new(document: DocumentRef) -> Self {
        Self {
            document,
            zoom: Cell::new(1.0),
            stale: Cell::new(true),
        }
    }

    /// What the editor draws with: the zoom, and the file's name at the
    /// root of the breadcrumb.
    pub fn look(&self) -> Look {
        Look {
            zoom: self.zoom.get(),
            root: self.document.title(),
        }
    }

    /// ⌘+, ⌘−, ⌘0.
    pub fn zoom(&self, ed: &Editor, z: ZoomCommand) {
        let now = match z {
            ZoomCommand::In => self.zoom.get() * ZOOM_STEP,
            ZoomCommand::Out => self.zoom.get() / ZOOM_STEP,
            ZoomCommand::Reset => 1.0,
        }
        .clamp(ZOOM_MIN, ZOOM_MAX);
        self.zoom.set(now);
        ed.set_look(self.look());
    }
}

impl Host for Shell {
    fn request(&self, ed: &Editor, request: Request) {
        match request {
            Request::Save => self.document.save(),
            Request::SaveAs => self.document.save_as(),
            Request::Zoom(z) => self.zoom(ed, z),
        }
    }

    fn read_clipboard(&self) -> Option<String> {
        platform::clipboard::read().text
    }

    fn repainted(&self, _model: &Model<FigBackend>) {
        self.stale.set(true);
    }
}

/// A data file's page: unit's root, and the editor under it.
#[allow(non_snake_case)]
pub fn DataPage() -> Element {
    let init = use_context::<DataInit>();
    let appearance = app::settings().appearance;
    rsx! {
        UnitRoot { keys: Keys::Host, appearance: unit_appearance(appearance),
            KeepUnit { cell: init.made.unit.clone() }
            Body {}
        }
    }
}

/// The editor, made under the root so that it finds the page's unit.
#[allow(non_snake_case)]
fn Body() -> Element {
    let init = use_context::<DataInit>();
    let unit = use_unit();
    let ed = use_editor(
        || {
            init.model
                .borrow_mut()
                .take()
                .expect("a data page is made once, with its model")
        },
        || init.shell.look(),
        || init.shell.clone() as Rc<dyn Host>,
    );
    use_hook(|| {
        *init.editor.borrow_mut() = Some(ed.clone());
        // A toast's and a long press's time.
        unit.set_timer(|d| Box::pin(tokio::time::sleep(d)));
    });
    let trailing = rsx! { Toolbar { editor: ed.clone() } };
    rsx! {
        // Redo is Undo's arrow turned round.
        style { ".flower-redo svg {{ transform: scaleX(-1); }}" }
        FlowerPages { editor: ed, trailing }
    }
}

/// The budgets on offer, after Fit: `(name, rows, depth)`.
const BUDGETS: [(&str, usize, usize); 3] = [("Pages", 6, 1), ("Roomy", 24, 2), ("Flat", 9999, 99)];

/// Which of the toolbar's budgets `budget` is: 0 for Fit.
fn budget_index(budget: Option<InlineBudget>) -> usize {
    budget
        .and_then(|b| {
            BUDGETS
                .iter()
                .position(|&(_, rows, depth)| InlineBudget::new(rows, depth) == b)
        })
        .map_or(0, |i| i + 1)
}

/// The budget the toolbar's `n`th option stands for.
fn budget_of(n: usize) -> Option<InlineBudget> {
    let (_, rows, depth) = *BUDGETS.get(n.checked_sub(1)?)?;
    Some(InlineBudget::new(rows, depth))
}

/// The buttons at the end of the editor's bar: how much of the file inlines
/// onto a page, and Undo, Redo and Delete.
///
/// How much inlines is about the room the pages are drawn in, not about any
/// one file, so it is the window's choice: Fit sizes it to the window and
/// follows a resize (the editor's default); Pages is the settings-menu rule;
/// Roomy inlines a couple of ranks; Flat puts the whole file on one page.
#[component]
fn Toolbar(editor: Editor) -> Element {
    let unit = use_unit();
    // Read so the buttons follow the model.
    let _ = editor.changed();
    let (normal, item, undo, redo) = {
        let m = editor.model();
        let normal = matches!(m.mode, Mode::Normal);
        (
            normal,
            normal && m.page_item().is_some(),
            normal && m.history_len() > 0,
            normal && m.redo_len() > 0,
        )
    };
    let selected = budget_index(editor.budget());
    let mut options = vec!["Fit".to_string()];
    options.extend(BUDGETS.iter().map(|(n, ..)| n.to_string()));
    let (b, u, r, d) = (
        editor.clone(),
        editor.clone(),
        editor.clone(),
        editor.clone(),
    );
    rsx! {
        Segmented { options, selected: Some(selected), label: "Inline",
            disabled: !normal,
            onchange: move |n: usize| {
                b.set_budget(budget_of(n));
                unit.blur();
            }
        }
        Button { variant: Variant::Ghost, class: "unit-icon", icon: paths::UNDO, label: "Undo",
                 disabled: !undo, refuses_keys: true, onpress: move |_| u.undo() }
        Button { variant: Variant::Ghost, class: "unit-icon flower-redo", icon: paths::UNDO,
                 label: "Redo", disabled: !redo, refuses_keys: true, onpress: move |_| r.redo() }
        Button { variant: Variant::Ghost, class: "unit-icon", icon: paths::TRASH, label: "Delete",
                 disabled: !item, refuses_keys: true,
                 onpress: move |_| {
                     let path = d.model().page_item().map(|i| i.path.clone());
                     if let Some(p) = path {
                         d.delete(p);
                     }
                 } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_budget_is_its_own_option_and_fit_is_none() {
        assert_eq!(budget_index(None), 0);
        assert_eq!(budget_of(0), None);
        for n in 1..=BUDGETS.len() {
            assert_eq!(budget_index(budget_of(n)), n);
        }
    }

    #[test]
    fn every_extension_reads_as_data_and_a_documents_does_not() {
        for ext in extensions() {
            assert!(
                flower_data::format::from_extension(ext).is_some(),
                "{ext} is a data format"
            );
        }
        assert!(model("a = 1\n", "toml").is_ok());
        assert!(model("# a heading\n", "md").is_err());
    }

    // What the page under test is made with, handed to it as the app's
    // window hands a root context.
    #[cfg(not(target_os = "macos"))]
    thread_local! {
        static INIT: RefCell<Option<DataInit>> = const { RefCell::new(None) };
    }

    #[cfg(not(target_os = "macos"))]
    #[allow(non_snake_case)]
    fn Hosted() -> Element {
        use_context_provider(|| INIT.with_borrow(|i| i.clone().expect("the test's init")));
        rsx! { DataPage {} }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_data_file_opens_in_a_page_and_a_key_edits_its_tree() {
        use dioxus::html::{Code, Key, Modifiers};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let source = "first = 1\nsecond = 2\n";
        std::fs::write(&path, source).unwrap();
        let document = platform::document::Document::read(&path).unwrap();
        let ext = document.data().expect("a data file");
        let init = DataInit::new(
            model(&document.take_source(), &ext).unwrap(),
            Shell::new(document),
        );
        INIT.with_borrow_mut(|i| *i = Some(init.clone()));
        let mut page = crate::headless::page(Hosted, 700.0, 500.0);
        let ed = init
            .editor
            .borrow()
            .clone()
            .expect("the page made its editor");
        assert_eq!(ed.look().root, "config.toml");
        assert_eq!(ed.model().source_snapshot(), source);
        assert!(!ed.model().dirty);
        // `x` deletes the entry the cursor is on, the first.
        let rt = page.vdom.runtime();
        rt.in_scope(dioxus_core::ScopeId::ROOT, || {
            assert!(ed.key(Key::Character("x".into()), Code::KeyX, Modifiers::empty()));
        });
        crate::headless::settle(&mut page);
        assert_eq!(ed.model().source_snapshot(), "second = 2\n");
        assert!(ed.model().dirty);
        assert!(init.shell.stale.get(), "the repaint told the shell");
    }

    #[test]
    fn text_that_does_not_parse_is_refused_with_a_reason() {
        let e = model("a = = 1\n", "toml").err().expect("refused");
        assert!(!e.is_empty());
    }
}
