# flower-dioxus

Flower on Dioxus's native renderer: `dioxus-native` 0.8.0-alpha.3, which
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
the picker and the prompts, out of
[unit](https://github.com/diaryx-org/unit)'s components, over a `Host` it
asks for what it does not own. Its README is how to host it. This app
(package `flower-dioxus-app`, binary `flower-dioxus`) is one host: the file
and the window around the editor.

- `main.rs` opens the file named on the command line, or the flower sample
  untitled when none is named or it will not open, under unit's root with
  the host's keys; gives the root the system clipboard and a sleep; and hands
  the editor the window's keys — what the editor does not take, the shell
  may.
- `toolbar.rs` is the buttons at the end of the editor's bar: how much of the
  document inlines onto a page (Fit, Pages, Roomy, Flat), Undo, Redo and
  Delete.
- `shell.rs` is the `Host`: ⌘S writes the file (a Save As sheet for an
  untitled document), ⇧⌘S asks where, ⌘+ / ⌘− / ⌘0 zoom the page, ⌘V reads
  the system clipboard, and the window title and the breadcrumb's root are
  the file's name, the title "— Edited" while it has unsaved changes. ⌘O
  opens another file, asking first about unsaved changes.
- `menu.rs` is the menu bar, through muda: Flower, File, Edit, Go, View and
  Window. Every item is a key chord replayed through the same handler as the
  keyboard, so the menu cannot disagree with it; a chord with ⌘ or ⌥ is
  shown as the item's shortcut, and the editor's single letters are not,
  since AppKit would take every `e` typed into a value for Edit Value. Items
  are enabled for what the editor is doing — the page's commands off while a
  value is open, Paste on only then, Undo and Redo by the journal. muda is
  built without GTK, so Linux shows no menu bar but builds and tests it.
- `file.rs` reads a file into a model, writes one back, and holds the dialogs
  — rfd's async ones, sheets on macOS, for the reason leaf-dioxus's `file.rs`
  gives. The unsaved-changes question is rfd's too: it has three answers,
  and unit's dialog two.

Both are workspaces of their own, like leaf's: `dioxus-native` is published
only as an alpha, and nothing in flower's root workspace should resolve its
winit beta and wgpu tree. CI's `dioxus` job lints and tests both.

**Blitz and winit's macOS backend are diaryx-org's fork's**, taken by git
in `[patch.crates-io]`: Blitz's accessibility tree reads unit's ARIA roles
and states; and with winit's backend unpatched, macOS's press-and-hold
accent menu never opens and the emoji picker's text is dropped. The editor turns the
input method on while a value is open (`crates/flower-dioxus/src/ime.rs`);
dead keys and composing input methods work without the patch too.

## On a Mac

`cargo xtask mac [FILE]` builds `Flower.app` around the binary and opens the
file through the Finder's route; `cargo xtask package` signs it for Developer
ID, notarises it and puts it in a `.dmg` (`xtask/src/package.rs`).

- `Info.plist` is the bundle's, stamped with the workspace version when it
  is put together. It declares JSON, YAML, TOML and fig as types Flower edits,
  as an alternate handler: Flower is in the Finder's Open With for each.
- `Flower.entitlements` is what the release is signed with: the sandbox, and
  the files the user opens.
- `finder.rs` hears the files the Finder asks the app to open — an Apple
  Event, not an argument — through an application delegate of the app's own,
  and the document's window opens the last of them as ⌘O would.

## Not yet

- **A window per document.** One window, one document; opening another
  replaces it, asking about unsaved changes first.
- **Autosave, Versions and Open Recent**, which the Swift app this replaced
  had from the system's document machinery.
- **Settings.** The look is the default's, zoomed, and the inline budget is
  the window's, not remembered.
- **An icon.**
