//! Flower on Dioxus's **native backend** (`dioxus-native` 0.8.0-alpha.3,
//! Blitz): the editor widget, [`flower_dioxus`], in a window, under unit's
//! root.
//!
//! ```sh
//! cargo run --release -- config.toml   # in apps/flower-dioxus
//! ```
//!
//! The editor — the panes, the keys, the picker, the prompts — is the
//! widget's (`crates/flower-dioxus`). This is what an app adds around it, the
//! two things flower-core deliberately does not own: the file, and the window.
//!
//! - [`shell`]: the `Host` — Save, Save As, the zoom, the window title — and
//!   Open (⌘O), the one key that is the app's and not the editor's.
//! - [`menu`]: the menu bar, every item a key chord replayed.
//! - [`toolbar`]: the buttons at the end of the editor's bar.
//! - [`file`]: reading a file into a model, and the dialogs.
//! - [`finder`]: the files the system asks the app to open.

mod file;
mod finder;
mod menu;
mod shell;
mod toolbar;

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::*;
use dioxus_native::winit;
use dioxus_native::winit::event::WindowEvent;
use flower_dioxus::{FlowerPages, Host, Look, pressed, use_editor};
use unit::{Accent, Clipboard, Keys, UnitRoot, use_unit};

use crate::shell::Shell;

fn main() {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("--version" | "-V") => {
            println!("flower-dioxus {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("--help" | "-h") => {
            println!("usage: flower-dioxus [config-file]");
            return;
        }
        _ => {}
    }
    // A Mac launching the app for a file it was asked to open passes a
    // `-psn_…` process serial number first on old systems; anything that is
    // not a path is no document.
    let path = arg.filter(|a| !a.starts_with("-psn_")).map(PathBuf::from);
    finder::install();
    dioxus_native::launch_cfg(
        App,
        vec![Box::new(move || Box::new(Opened(path.clone())))],
        vec![],
    );
}

/// The file named on the command line, if any.
#[derive(Clone)]
struct Opened(Option<PathBuf>);

#[allow(non_snake_case)]
fn App() -> Element {
    rsx! {
        UnitRoot { keys: Keys::Host, accent: Accent::System, Document {} }
    }
}

/// The system clipboard, for a prompt's field.
struct System;

impl Clipboard for System {
    fn read(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    fn write(&self, text: &str) {
        if let Ok(mut c) = arboard::Clipboard::new() {
            let _ = c.set_text(text);
        }
    }
}

/// The document's window: under the root, so the editor and the toolbar
/// find the page's unit.
#[allow(non_snake_case)]
fn Document() -> Element {
    let path = use_context::<Opened>().0;
    let window = dioxus_native::use_window();
    let unit = use_unit();
    let (model, opened) = file::open_or_sample(path);
    let root = file::name_of(opened.as_deref());
    let shell = use_hook(|| Rc::new(Shell::new(window, opened)));
    let ed = use_editor(
        || model,
        || Look {
            root,
            ..Look::default()
        },
        || shell.clone() as Rc<dyn Host>,
    );

    // What unit asks of a host that reads the keys itself: the clipboard for
    // a prompt's field, and a sleep to time a toast and a long press.
    use_hook(move || {
        unit.set_clipboard(Rc::new(System));
        unit.set_timer(|d| Box::pin(tokio::time::sleep(d)));
    });

    // The menu bar, installed once; a task takes what its items ask for and
    // keeps them enabled for what the editor is doing. muda queues each
    // choice, and a shortcut that names an item is the item's — macOS gives
    // it to the menu before the view — so this is where most ⌘ chords arrive.
    use_hook({
        let (ed, shell) = (ed.clone(), shell.clone());
        move || {
            let menu = menu::install();
            spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    while let Some(menu::Chord(key, code, mods)) = menu.next() {
                        shell.chord(&ed, key, code, mods);
                    }
                    // One window, one document: of several files asked for
                    // at once, the last.
                    if let Some(asked) = finder::take().pop() {
                        shell.open_path(&ed, asked);
                    }
                    menu.enable(menu::Enabled::of(&ed.model()));
                }
            });
        }
    });

    // The window's keys go the same way as the menu's; the rest of its
    // events are the editor's.
    let mut mods = winit::keyboard::ModifiersState::empty();
    dioxus_native::use_window_event({
        let (ed, shell) = (ed.clone(), shell.clone());
        move |event, _| match event {
            WindowEvent::ModifiersChanged(m) => mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some((key, code, m)) = pressed(event, mods) {
                    shell.chord(&ed, key, code, m);
                }
            }
            event => ed.window_event(event),
        }
    });

    let trailing = rsx! { toolbar::Toolbar { editor: ed.clone() } };
    rsx! {
        // Redo is Undo's arrow turned round.
        style { ".flower-redo svg {{ transform: scaleX(-1); }}" }
        FlowerPages { editor: ed, trailing }
    }
}
