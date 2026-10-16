//! Flower on Dioxus's **native backend** (`dioxus-native` 0.8.0-alpha.2,
//! Blitz): the editor widget, [`flower_dioxus`], in a window.
//!
//! ```sh
//! cargo run --release -- config.toml   # in apps/flower-dioxus
//! ```
//!
//! The editor — the panes, the keys, the picker, the edit line — is the
//! widget's (`crates/flower-dioxus`). This is what an app adds around it, the
//! two things flower-core deliberately does not own: the file, and the window.
//!
//! - [`shell`]: the `Host` — Save, Save As, the zoom, the window title — and
//!   Open (⌘O), the one key that is the app's and not the editor's.
//! - [`file`]: reading a file into a model, and the dialogs.

mod file;
mod shell;

use std::path::PathBuf;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_native::winit;
use dioxus_native::winit::event::WindowEvent;
use flower_dioxus::{Host, pressed, use_editor};

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
    let path = arg.map(PathBuf::from);
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
    let path = use_context::<Opened>().0;
    let window = dioxus_native::use_window();
    let (model, opened) = file::open_or_sample(path);
    let shell = use_hook(|| Rc::new(Shell::new(window, opened)));
    let ed = use_editor(
        || model,
        flower_dioxus::Look::default,
        || shell.clone() as Rc<dyn Host>,
    );

    // The keys, which the editor sees first: what it does not take, the
    // shell may (⌘O). The rest of the window's events are the editor's.
    let mut mods = winit::keyboard::ModifiersState::empty();
    dioxus_native::use_window_event({
        let (ed, shell) = (ed.clone(), shell.clone());
        move |event, _| match event {
            WindowEvent::ModifiersChanged(m) => mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some((key, code, m)) = pressed(event, mods)
                    && !ed.key(key.clone(), code, m)
                {
                    shell.key(&ed, &key, m);
                }
            }
            event => ed.window_event(event),
        }
    });

    ed.render()
}
