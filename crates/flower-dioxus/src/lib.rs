//! flower-dioxus — an **embeddable** structural config editor for Dioxus's
//! native backend (`dioxus-native` 0.8.0-alpha.3, Blitz): an HTML/CSS
//! renderer on wgpu, with no webview and no browser engine.
//!
//! The page view of `flower-ratatui` and of the Swift app it replaces, drawn
//! as a settings screen out of [unit](https://github.com/diaryx-org/unit)'s
//! components: one container at a time, two panes when there is width and
//! depth for them, a section per group, short lists as chips, a switch for a
//! boolean and a menu for a field with a vocabulary, values typed in place,
//! the picker in a modal, and every row's operations in its context menu.
//! flower-core owns the model — the projection, the cursor, every lossless
//! edit — and this draws it and hands it intents, as the other frontends do.
//!
//! A host makes an [`Editor`] with [`use_editor`] in a component under a
//! [`unit::UnitRoot`] with `keys: Keys::Host`, puts [`FlowerPages`] (or
//! [`Editor::render`]) in its page, and hands the editor the window's events
//! ([`Editor::key`] through [`pressed`], and [`Editor::window_event`]). What
//! the editor cannot do by itself it asks of the host's [`Host`]: Save, Save
//! As and the zoom ([`Request`]), the clipboard, and a word after every
//! repaint.
//!
//! Where to look:
//!
//! - [`editor`]: the state, the geometry, and every way in — keys, the
//!   window's events, presses, the wheel, the prompts.
//! - [`input`]: the key table.
//! - [`layout`]: the panes, a page's sections and rows, what each row is
//!   called, and how tall everything is — no Dioxus type, so all of it
//!   tested without a window.
//! - [`view`]: the stylesheet and the markup.
//! - [`ime`]: the input method, into the open field.
//! - [`host`], [`keys`].
//!
//! The editor fills the window but for the host's chrome above and below it
//! ([`Editor::set_bounds`]): keys come from winit rather than DOM focus
//! (`keys.rs` says why), and the room a page has is computed rather than
//! measured.

pub mod editor;
pub mod host;
pub mod ime;
pub mod input;
pub mod keys;
pub mod layout;
pub mod view;

pub use editor::{Bounds, Editor, Geometry, Look, use_editor};
pub use host::{Host, Request};
pub use input::ZoomCommand;
pub use keys::pressed;
pub use view::{FlowerPages, css};
