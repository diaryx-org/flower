//! What the editor asks of whoever embeds it: the [`Host`].
//!
//! The editor owns the model's navigation and every edit. What it does not
//! own is the world around it — what Save means, the clipboard, the window's
//! title — so it asks. Every method has a default, so `struct Mine; impl
//! Host for Mine {}` is a host: Save, Save As and the zoom ignored, and no
//! clipboard to paste from.
//!
//! flower-core is filesystem-free and so is this crate: the editor never reads
//! or writes a document. Save is a [`Request`], and the host writes
//! [`Model::source_snapshot`](flower_core::Model::source_snapshot) wherever the
//! document came from — a file, or the frontmatter of another document — then
//! tells the model how it went with
//! [`Model::mark_saved`](flower_core::Model::mark_saved) and
//! [`Model::set_status`](flower_core::Model::set_status).

use flower_core::{Backend, FigBackend, Model};

use crate::editor::Editor;
use crate::input::ZoomCommand;

/// A command the editor recognises — its key is one of the editor's — but
/// leaves to the host to carry out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// ⌘S: write the document where it came from.
    Save,
    /// ⇧⌘S: name a place for the document and write it there.
    SaveAs,
    /// ⌘+ (or ⌘=), ⌘−, ⌘0. The host owns the zoom because it owns the
    /// [`Look`](crate::Look) it scales.
    Zoom(ZoomCommand),
}

/// The editor's host.
///
/// [`request`](Self::request) is called with nothing of the editor's
/// borrowed, so it may read or change the model ([`Editor::model`]) and
/// repaint. The others may be called while the model is borrowed, and must
/// not call back into the editor.
pub trait Host<B: Backend + 'static = FigBackend> {
    /// Carry out a command the editor leaves to the host.
    fn request(&self, ed: &Editor<B>, request: Request) {
        let _ = (ed, request);
    }

    /// What the clipboard holds as text, for ⌘V into an open value. The
    /// editor takes its first line.
    fn read_clipboard(&self) -> Option<String> {
        None
    }

    /// The editor has just repainted `model`: the moment to bring anything
    /// outside the editor up to date with it — a window title, a dirty dot,
    /// or, for a host editing another document's frontmatter, that document.
    fn repainted(&self, model: &Model<B>) {
        let _ = model;
    }
}
