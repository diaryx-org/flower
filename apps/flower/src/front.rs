//! A document's frontmatter, shown in its window in place of the document:
//! the YAML (or TOML) between the fences at its head, read as a tree by
//! flower-data-editor, as a data file's window reads a file (`data.rs`).
//!
//! The document's views draw none of it, and its source view draws it as
//! text. This is the third way to see it, there only for a document that has
//! some: View ▸ Frontmatter, and the toolbar's button, put the structural
//! editor over the document's area and give it the keys, and take it away
//! again. Its toolbar and its file stay the document's.
//!
//! The frontmatter is the document's text and stays so. The editor's model
//! is made from it when the view is shown, and every edit is written back
//! between the fences as one edit to the document ([`Shell::repainted`]),
//! so Save writes it and the title shows it unsaved like any other. While
//! the view is shown, Undo and Redo are the structural editor's; once it is
//! gone, each of its edits is a step in the document's own history.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use dioxus::prelude::*;
use flower_data::{FigBackend, Model};
use flower_data_editor::{Bounds, Editor, FlowerPages, Host, Look, Request, use_editor};
use flower_doc::{Doc, Format, TopKind};

use crate::page::{DocInit, chrome_height};
use crate::platform;

/// Where a document's frontmatter is: the text between its fences, and the
/// extension that names the format it is in to `data::model`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub inner: Range<usize>,
    pub extension: &'static str,
}

/// The frontmatter at the head of `source`, by its fences: `---` for YAML,
/// `+++` for TOML, closed by the same on a line of its own.
pub fn within(source: &str) -> Option<Found> {
    let open = source.find('\n')? + 1;
    let fence = source[..open].trim_end();
    let extension = match fence {
        "---" => "yaml",
        "+++" => "toml",
        _ => return None,
    };
    let mut at = open;
    for line in source[open..].split_inclusive('\n') {
        if line.trim_end() == fence {
            return Some(Found {
                inner: open..at,
                extension,
            });
        }
        at += line.len();
    }
    None
}

/// Whether `doc` has frontmatter to show, asked after every repaint: by
/// its fences alone, which costs the frontmatter's length and no more.
pub fn has(doc: &Doc) -> bool {
    doc.format != Format::Html && within(&doc.source).is_some()
}

/// `doc`'s frontmatter, where its parse and its fences agree there is some.
pub fn find(doc: &mut Doc) -> Option<Found> {
    if doc.format == Format::Html {
        return None;
    }
    let found = within(&doc.source)?;
    doc.top_level()
        .iter()
        .any(|b| b.kind == TopKind::Frontmatter && b.span.start == 0)
        .then_some(found)
}

/// What a document's page keeps of its frontmatter's view.
#[derive(Default)]
pub struct Front {
    /// Whether the view is shown.
    pub shown: Cell<bool>,
    /// The model the view is next made with, and the text it was read from.
    pending: RefCell<Option<(Model<FigBackend>, String)>>,
    /// The view's editor, while it is shown.
    pub editor: RefCell<Option<Editor>>,
}

impl Front {
    /// Read `doc`'s frontmatter for the view to be made with; or why there
    /// is none to show, in a line.
    pub fn prepare(&self, doc: &mut Doc) -> Result<(), String> {
        let found = find(doc).ok_or("This document has no frontmatter")?;
        let text = doc.source[found.inner.clone()].to_string();
        let model = crate::data::model(&text, found.extension)
            .map_err(|e| format!("The frontmatter does not parse: {e}"))?;
        *self.pending.borrow_mut() = Some((model, text));
        Ok(())
    }

    /// The view has gone, or is to.
    pub fn hide(&self) {
        self.shown.set(false);
        self.pending.borrow_mut().take();
    }
}

/// The view's [`Host`]: the document the frontmatter is part of.
struct Shell {
    init: DocInit,
    /// The frontmatter as last read from the document or written to it.
    written: RefCell<String>,
}

impl Host for Shell {
    fn request(&self, _ed: &Editor, request: Request) {
        match request {
            Request::Save => self.init.shell.document.save(),
            Request::SaveAs => self.init.shell.document.save_as(),
            Request::Zoom(_) => {}
        }
    }

    fn read_clipboard(&self) -> Option<String> {
        platform::clipboard::read().text
    }

    /// Write what the model now says between the document's fences, as one
    /// edit to the document; the chrome reads the document again after.
    fn repainted(&self, model: &Model<FigBackend>) {
        self.init.shell.stale.set(true);
        let mut text = model.source_snapshot();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        if *self.written.borrow() == text {
            return;
        }
        let Ok(mut doc) = self.init.doc.try_borrow_mut() else {
            return;
        };
        let Some(next) = spliced(&doc.source, &text) else {
            return;
        };
        if doc.replace_source(&next) {
            *self.written.borrow_mut() = text;
        }
    }
}

/// `source` with `text` between its frontmatter's fences; `None` if it has
/// none.
fn spliced(source: &str, text: &str) -> Option<String> {
    let found = within(source)?;
    let mut next = String::with_capacity(source.len() + text.len());
    next.push_str(&source[..found.inner.start]);
    next.push_str(text);
    next.push_str(&source[found.inner.end..]);
    Some(next)
}

/// The view: the structural editor over the document's area, below the
/// toolbar. Made when the view is shown, with what [`Front::prepare`] read.
#[allow(non_snake_case)]
pub fn Pane() -> Element {
    let init = use_context::<DocInit>();
    let ed = use_editor(
        || {
            let pending = init.front.pending.borrow_mut().take();
            pending.expect("the view is shown once its model is read").0
        },
        || Look {
            zoom: 1.0,
            root: "Frontmatter".into(),
        },
        || {
            let written = init.doc.try_borrow().ok().and_then(|d| {
                let found = within(&d.source)?;
                Some(d.source[found.inner].to_string())
            });
            Rc::new(Shell {
                init: init.clone(),
                written: RefCell::new(written.unwrap_or_default()),
            }) as Rc<dyn Host>
        },
    );
    use_hook(|| {
        ed.set_bounds(Bounds {
            top: chrome_height(),
            bottom: 0.0,
        });
        *init.front.editor.borrow_mut() = Some(ed.clone());
    });
    use_drop({
        let front = init.front.clone();
        move || {
            front.editor.borrow_mut().take();
        }
    });
    let top = chrome_height();
    rsx! {
        div { style: "position: fixed; left: 0; right: 0; top: {top}px; bottom: 0; z-index: 2; \
                      background: var(--leaf-bg); overflow: hidden;",
            FlowerPages { editor: ed }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_is_what_lies_between_the_fences_at_the_head() {
        let src = "---\ntitle: A note\ntags: [a]\n---\n# A note\n";
        let f = within(src).unwrap();
        assert_eq!(&src[f.inner.clone()], "title: A note\ntags: [a]\n");
        assert_eq!(f.extension, "yaml");
        assert_eq!(within("+++\na = 1\n+++\n").unwrap().extension, "toml");
        assert_eq!(within("# A note\n\n---\n\ntext\n"), None);
        assert_eq!(within("---\nnever: closed\n"), None);
        assert_eq!(within(""), None);
    }

    #[test]
    fn a_document_with_frontmatter_has_some_and_one_without_has_none() {
        let mut with =
            Doc::from_source("---\ntitle: T\n---\n# T\n".into(), Format::Markdown).unwrap();
        assert!(has(&with));
        assert_eq!(find(&mut with).unwrap().inner, 4..13);
        let mut without = Doc::from_source("# T\n\ntext\n".into(), Format::Markdown).unwrap();
        assert!(!has(&without));
        assert_eq!(find(&mut without), None);
        // A rule and a heading under it are not frontmatter.
        let rule = Doc::from_source("text\n\n---\n\nmore\n".into(), Format::Markdown).unwrap();
        assert!(!has(&rule));
    }

    #[test]
    fn an_edit_is_written_between_the_fences_and_nowhere_else() {
        let src = "---\ntitle: T\n---\n# T\n\nbody\n";
        assert_eq!(
            spliced(src, "title: New\ndraft: true\n").unwrap(),
            "---\ntitle: New\ndraft: true\n---\n# T\n\nbody\n"
        );
        assert_eq!(spliced(src, "").unwrap(), "---\n---\n# T\n\nbody\n");
        assert_eq!(spliced("# T\n", "a: 1\n"), None);
    }

    #[test]
    fn frontmatter_that_does_not_parse_is_not_shown_and_says_why() {
        let front = Front::default();
        let mut doc =
            Doc::from_source("---\na: [1, 2\n---\n# T\n".into(), Format::Markdown).unwrap();
        let e = front.prepare(&mut doc).unwrap_err();
        assert!(e.starts_with("The frontmatter does not parse"), "{e}");
        let mut ok = Doc::from_source("---\na: 1\n---\n# T\n".into(), Format::Markdown).unwrap();
        assert!(front.prepare(&mut ok).is_ok());
    }
}
