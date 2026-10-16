# flower-dioxus

Flower on Dioxus's native renderer: `dioxus-native` 0.8.0-alpha.2, which
draws HTML and CSS through Blitz on wgpu, with no webview and no browser
engine. It edits the same files as the terminal app and the Swift one,
through `flower-core`.

```sh
cargo run --manifest-path apps/flower-dioxus/Cargo.toml -- config.toml
cargo test --manifest-path apps/flower-dioxus/Cargo.toml
cargo test --manifest-path crates/flower-dioxus/Cargo.toml   # the editor
```

**Two crates, as leaf's Dioxus editor is.** The editor is a widget,
[`crates/flower-dioxus`](../../crates/flower-dioxus): the panes, the keys,
the picker and the edit line, over a `Host` it asks for what it does not own
and a `Look` it draws with. Its README is how to host it. This app (package
`flower-dioxus-app`, binary `flower-dioxus`) is one host: the file and the
window around the editor.

- `main.rs` opens the file named on the command line, or the flower sample
  untitled when none is named or it will not open, and hands the editor the
  window's keys — what the editor does not take, the shell may.
- `shell.rs` is the `Host`: ⌘S writes the file (a Save As sheet for an
  untitled document), ⇧⌘S asks where, ⌘+ / ⌘− / ⌘0 zoom the type, ⌘V reads
  the system clipboard, and the window title is the file's name, "— Edited"
  while it has unsaved changes. ⌘O opens another file, asking first about
  unsaved changes.
- `file.rs` reads a file into a model, writes one back, and holds the dialogs
  — rfd's async ones, sheets on macOS, for the reason leaf-dioxus's `file.rs`
  gives.

Both are workspaces of their own, like leaf's: `dioxus-native` is published
only as an alpha, and nothing in flower's root workspace should resolve its
winit beta and wgpu tree.

## Not yet

- **A menu bar.** Every command is a key; leaf-dioxus's `menu.rs` (muda,
  every editor item a key chord replayed) is the model to follow.
- **A patched winit.** Plain typing works unpatched; press-and-hold accents
  and the emoji picker on macOS need leaf's `patches/winit-appkit`, which a
  `[patch]` here would have to vendor again — the proposal for a shared,
  patched winit in leaf's `docs/proposals/` is the better road.
- **Settings.** The look is the default's, zoomed.
