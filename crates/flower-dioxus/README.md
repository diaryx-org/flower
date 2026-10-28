# flower-dioxus

The page editor for Dioxus's native renderer: `dioxus-native` 0.8.0-alpha.3,
which draws HTML and CSS through Blitz on wgpu, with no webview and no
browser engine. It is flower's page view as a settings screen, built from
[unit](https://github.com/diaryx-org/unit)'s components, over the same
`flower_core::Model` the terminal edits:

- one container at a time, two panes when there is width and depth for them,
  the left the page the right was opened from;
- a section per group, under its caption, and a scalar list as one row of
  chips;
- a switch for a boolean, a menu for a field with a vocabulary, and every
  other value typed in place in its row;
- the demoted fields folded behind an "Advanced" disclosure;
- the picker in a modal, Rename and Add a Field as prompts, and every row's
  operations in its context menu;
- a refusal said under the row it refused, or as a toast when there is no
  such row on screen.

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
use flower_dioxus::{FlowerPages, Host, Look, pressed, use_editor};
use unit::{Keys, UnitRoot};

struct Mine;
impl Host for Mine {} // every request ignored

#[allow(non_snake_case)]
fn App() -> Element {
    rsx! { UnitRoot { keys: Keys::Host, Page {} } }
}

#[allow(non_snake_case)]
fn Page() -> Element {
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
    rsx! { FlowerPages { editor: ed } }
}
```

- **Under unit's root, with the host's keys.** The editor's controls are
  unit's, so it is made in a component under a `UnitRoot` with
  `keys: Keys::Host`, whose overlay draws its menus, prompts and toasts.
  `Editor::key` offers each key to the page's controls first — an open menu,
  a prompt's field — and then to the editor's table.
- **`use_editor`** makes the editor from the model, the [look](#the-look) and
  the [host](#the-host), each made once. It puts the model in the page
  projection, fits its inline budget to the window and opens it at the first
  page that says something. `Editor::install` puts in another model.
- **The window's events** are the host's to hand over (`keys.rs` says why).
  `Editor::key` answers whether the key was taken, so a host can offer the
  rest to its own commands; `Editor::window_event` takes the surface's size,
  the wheel, the theme and an input method's commit.
- **`FlowerPages { editor, trailing }`** draws it; `trailing` is the host's
  own buttons at the end of the breadcrumb's bar. A host with chrome above or
  below the editor says how tall it is with `Editor::set_bounds`.
- **`Editor::model`** reads or changes the model; `Editor::refresh` draws the
  change, and `Editor::changed` subscribes a host's component to it.
- **`Editor::set_budget`** fixes how much inlines onto a page; `None`, the
  default, fits it to the window.

The editor is generic over flower-core's `Backend`, defaulting to
`FigBackend`, so `Editor` and `Host` name the fig-backed model unless a host
says otherwise. `FlowerPages` is for the fig-backed model; any other draws
with `Editor::render`.

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

The pointer does what a settings window's does: a press on a container opens
it, a press on a value opens it for typing (a press elsewhere commits it), a
switch and a menu set their field, a chip is pressed to edit it and its ×
deletes it, and the secondary button opens the row's menu — Edit, Rename…,
the comments, Add, Move Up and Down, Delete. A press on the left pane backs
out onto the row pressed and opens it. The wheel scrolls the pane under the
pointer; the keys scroll a pane to follow the cursor.

## The host

| Method | When | Default |
|---|---|---|
| `request` | ⌘S `Save`, ⇧⌘S `SaveAs`, ⌘+ / ⌘− / ⌘0 `Zoom` | ignored |
| `read_clipboard` | ⌘V while a value is open | nothing |
| `repainted` | after every change, with the model: a window title, a dirty dot, the document the model's text belongs in | nothing |

Like flower-core, the editor never touches a file. Save is a request: the
host writes `Model::source_snapshot` wherever the document came from, then
calls `Model::mark_saved` and `Model::set_status`.

A host also gives the page's unit what it asks of any host that reads its
keys: the clipboard for a prompt's field (`Unit::set_clipboard`), and a sleep
(`Unit::set_timer`), which times a toast.

## The look

A `Look` is the zoom and what the document's root is called in the
breadcrumb (a file's name). The colours, the face and light or dark are the
page's: the `UnitRoot`'s theme and appearance, the same for every control in
the window. The editor adds only a colour per value's kind and the cursor's
row, for both appearances.

Every size is the layout's `Sizes`, scaled by the zoom, which the stylesheet
writes and the layout adds up — unit's own sizes are set on the editor's box
from the same numbers, so a zoom scales its controls too. Blitz cannot be
asked how tall a row came out, so every row's lines are fixed, one line each
and cut with an ellipsis, and a pane's scroll and the input method's caret
are computed from them.

## What a host has to know

- **The editor owns the window's input method.** It turns it on while a
  value is open and off otherwise (on a page every letter is a command),
  draws a composition underlined in the open field, and tells the input
  method the text before the caret (`ime.rs`). Dead keys and composing input
  methods work on any winit; press-and-hold accents and the emoji picker on
  macOS need the patched `winit-appkit` in diaryx-org's Blitz fork, which a
  host takes in its own `[patch.crates-io]`, as `apps/flower-dioxus` does.
- **Blitz from the fork.** unit's controls are read by assistive tech, and
  pressed by it, through the fork's accessibility tree; a host patches the
  Blitz crates to diaryx-org's fork as this crate's manifest does.
- **An open value is drawn, not an `<input>`.** A page under Blitz cannot
  give an `<input>` the focus from code, and the editor's keys come from the
  window, so the open field is the model's buffer drawn with a caret.
