# flower-dioxus

The page editor for Dioxus's native renderer: `dioxus-native` 0.8.0-alpha.3,
which draws HTML and CSS through Blitz on wgpu, with no webview and no
browser engine. It is `flower-ratatui`'s page view in a window — one
container at a time, two panes when there is width and depth for them, small
groups inlined under captions, short lists as chips, the edit line in the
footer and the picker over the page — over the same `flower_core::Model`.
`apps/flower-dioxus` is one host for it; leaf's Dioxus editor, showing a
document's frontmatter, is meant to be another.

```sh
cargo test --manifest-path crates/flower-dioxus/Cargo.toml
cargo run  --manifest-path crates/flower-dioxus/Cargo.toml --example minimal
```

A workspace of its own, not a member of flower's root one, for the reason
leaf's `crates/leaf-dioxus` is: `dioxus-native` is an alpha that pins a winit
beta and a large wgpu tree nothing in the root workspace should resolve.

## Hosting it

```rust
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_native::winit::event::WindowEvent;
use flower_dioxus::{Host, Look, pressed, use_editor};

struct Mine;
impl Host for Mine {} // every request ignored

#[allow(non_snake_case)]
fn App() -> Element {
    let ed = use_editor(
        || flower_core::Model::new(backend()).unwrap(),
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
    ed.render()
}
```

- **`use_editor`** makes the editor in the calling component, from the model,
  the [look](#the-look) and the [host](#the-host), each made once. It puts
  the model in the page projection, fits its inline budget to the window and
  opens it at the first page that says something — what `flower-tui` does
  before its first frame. `Editor::install` puts in another model.
- **The window's events** are the host's to hand over, because Blitz's DOM
  focus is not reliable enough for an editor (`keys.rs`). `Editor::key`
  answers whether the key was the editor's, so a host can offer the rest to
  its own commands; `Editor::window_event` takes the surface's size, the
  theme and an input method's commit.
- **`Editor::model`** reads or changes the model; `Editor::refresh` draws the
  change.

The editor is generic over flower-core's `Backend`, defaulting to
`FigBackend`, so `Editor` and `Host` name the fig-backed model unless a host
says otherwise.

## The keys

The terminal's table, so a reader moves between the two without relearning
it, and what a window adds:

| Key | What it does |
|-----|--------------|
| `j` / `k`, ↓ / ↑ | next / previous item on the page |
| `l`, →, Return, Space, ⌘↓ | open the container as a page; on a value, edit it |
| `h`, ←, Esc, ⌘↑ | back to the page that listed this one |
| `e` / `E` | pick a value where the field has a vocabulary, else edit it / edit it as text |
| `a` / `A` | add to the list the cursor is on or in, from the picker / as text |
| `c` / `C` | edit the comment after the value / the block above it |
| `x`, ⌦, ⌘⌫ | delete the entry or item |
| ⌥↑ / ⌥↓ | move it up / down |
| `u` / `U`, ⌘Z / ⇧⌘Z | undo / redo |
| ⌘S, ⇧⌘S, ⌘+ ⌘− ⌘0 | Save, Save As, the zoom — the host's ([`Request`](src/host.rs)) |

With a value open, every printable key types (⌘V pastes the host's
clipboard), Return commits and Esc cancels; in the picker, typing filters,
`j`/`k` and the arrows walk, Return chooses. Any other chord with ⌘ or ⌃ is
not the editor's.

A press on an item stands on it, and a press on the item the cursor stands
on opens it; a press on the left pane backs out onto the item pressed, and on
the preview opens it there. A press on a chip stands on that member. The
wheel walks the cursor, a line's travel a step, as the terminal's does — the
list scrolls by following the cursor, so there is one scroll position and it
is always where the cursor is.

## The host

| Method | When | Default |
|---|---|---|
| `request` | ⌘S `Save`, ⇧⌘S `SaveAs`, ⌘+ / ⌘− / ⌘0 `Zoom` | ignored |
| `read_clipboard` | ⌘V while a value is open | nothing |
| `repainted` | after every change, with the model: a window title, a dirty dot, the document the model's text belongs in | nothing |

Like flower-core, the editor never touches a file. Save is a request: the
host writes `Model::source_snapshot` wherever the document came from, then
calls `Model::mark_saved` and `Model::set_status`.

## The look

A `Look` is the monospace face (a CSS family list), its size and the
appearance — light, dark, or the system's. The page is laid out in cells, as
the terminal's is, so names read as one column and values as another; how
many cells a pane holds is its width over the face's advance, taken as
0.61 em (`CELL_EM`), which every common system monospace face is at or just
under. A face much wider than that would have a long value run past its
pane's edge, where measuring it as leaf-dioxus does with parley would not.

## What a host has to know

- **The editor assumes the window is its own**, as leaf-dioxus does: its
  stylesheet clips at `body`, and the room a page has is the window's surface
  less the footer. Sharing a window — a frontmatter panel above a document —
  needs `Geometry` to take the editor's box rather than the window's.
- **The editor owns the window's input method.** It turns it on while a
  value is open and off otherwise (on a page every letter is a command), draws
  a composition underlined after the buffer, and tells the input method the
  text before the caret (`ime.rs`). Dead keys and composing input methods
  work on any winit; press-and-hold accents and the emoji picker on macOS
  need leaf's patched `winit-appkit`, which a host takes in its own
  `[patch.crates-io]`, as `apps/flower-dioxus` does.
