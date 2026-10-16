//! flower-dioxus — an **embeddable** structural config editor for Dioxus's
//! native backend (`dioxus-native` 0.8.0-alpha.2, Blitz): an HTML/CSS
//! renderer on wgpu, with no webview and no browser engine.
//!
//! The page view of `flower-ratatui` and the Swift `FlowerPages`, in a
//! window: one container at a time, two panes when there is width and depth
//! for them, small groups inlined under captions, short lists as chips, every
//! value edited in a footer that doubles as the edit line, and the picker over
//! the page where a field has a vocabulary. flower-core owns the model — the
//! projection, the cursor, every lossless edit — and this draws it and hands
//! it intents, as the other two frontends do.
//!
//! A host makes an [`Editor`] with [`use_editor`] in its component, puts
//! [`Editor::render`] in its page, and hands the editor the window's events
//! ([`Editor::key`] through [`pressed`], and [`Editor::window_event`]). What
//! the editor cannot do by itself it asks of the host's [`Host`]: Save, Save
//! As and the zoom ([`Request`]), the clipboard, and a word after every
//! repaint. It draws with the [`Look`] the host gives it.
//!
//! Where to look:
//!
//! - [`editor`]: the state, the geometry, and every way in — keys, the
//!   window's events, presses, the wheel.
//! - [`input`]: the key table.
//! - [`layout`]: the panes, the entries of a page and where they land —
//!   no Dioxus type, so all of it tested without a window.
//! - [`view`]: the stylesheet, light and dark, and the markup.
//! - [`host`], [`keys`].
//!
//! Like leaf-dioxus, the editor assumes the window is its own: keys come from
//! winit rather than DOM focus (`keys.rs` says why), the body clips the page,
//! and the room a page has is the window's surface less the footer.

pub mod editor;
pub mod host;
pub mod input;
pub mod keys;
pub mod layout;
pub mod view;

pub use editor::{Editor, Geometry, Look, use_editor};
pub use host::{Host, Request};
pub use input::ZoomCommand;
pub use keys::pressed;
pub use view::{Appearance, Palette, css, themed};
