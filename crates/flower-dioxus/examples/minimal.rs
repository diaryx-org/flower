//! The smallest host: the editor alone in a window, on a document in memory,
//! with every request ignored.
//!
//! ```sh
//! cargo run --manifest-path crates/flower-dioxus/Cargo.toml --example minimal
//! ```

use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_native::winit::event::WindowEvent;
use flower_core::{FigBackend, Model};
use flower_dioxus::{FlowerPages, Host, Look, pressed, use_editor};
use unit::{Keys, UnitRoot};

struct Mine;
impl Host for Mine {}

const SAMPLE: &str = "\
# A sample config
title = \"flower\"
version = 1
debug = false

[server]
host = \"localhost\" # dev only
port = 8080
tags = [\"alpha\", \"beta\"]
";

fn main() {
    dioxus_native::launch(App);
}

#[allow(non_snake_case)]
fn App() -> Element {
    rsx! {
        UnitRoot { keys: Keys::Host, Page {} }
    }
}

/// Under the root, so the editor finds the page's unit.
#[allow(non_snake_case)]
fn Page() -> Element {
    let ed = use_editor(
        || {
            let backend = FigBackend::open(SAMPLE.as_bytes(), fig_toml::format().unwrap())
                .expect("a document");
            Model::new(backend).expect("a model")
        },
        Look::default,
        || Rc::new(Mine) as Rc<dyn Host>,
    );
    let mut mods = Default::default();
    dioxus_native::use_window_event({
        let ed = ed.clone();
        move |event, _| match event {
            WindowEvent::ModifiersChanged(m) => mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some((key, code, m)) = pressed(event, mods) {
                    ed.key(key, code, m);
                }
            }
            event => ed.window_event(event),
        }
    });
    rsx! { FlowerPages { editor: ed } }
}
