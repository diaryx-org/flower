---
part_of: id:dx4hxn744/kv2bv2m
title: flower
contents:
- '[AGENTS](/AGENTS.md)'
- '[Sample](sample.md)'
- '[flower on diaryx.org](/www/index.md)'
- '[Audiences](/vocab/audiences.md)'
- '[Proposals](/docs/proposals/proposals.md)'
- '[Tasks](/docs/tasks/tasks.md)'
- '[Releasing flower](/docs/releasing.md)'
- '[Changelog of leaf, to 0.5.1](/docs/CHANGELOG-leaf.md)'
- '[Archive](/archive/README.md)'
config: .config/prov.yaml
registry: registry.yaml
id: rkhvqqw
slug: flower
---
# flower (Work in progress!!)

An editor for the files a person keeps: **documents** (Markdown, Djot, HTML)
under an ordinary caret, and **data files** (JSON, YAML, TOML, ZON, figl) as a
tree edited a value at a time. Either way the file stays a live, lossless
syntax tree while it is edited: a document's is
[`twig`](https://github.com/diaryx-org/twig)'s, a data file's
[`fig`](https://github.com/diaryx-org/fig)'s, and what an edit does not touch
is written back byte for byte.

flower was two projects. **leaf** was the document editor, and most of what
is here is its code; **flower** was the structural editor for config files.
They are one now, under flower's name and leaf's icon, and one app opens
both:

```sh
cargo run -p flower-app -- notes.md       # a document: the caret, the toolbar, Reflow and Pages
cargo run -p flower-app -- config.toml    # a data file: its tree, a page per container
```

A document with frontmatter has both halves in one window: **View ▸
Frontmatter** (and the toolbar's button) shows the fields between its fences
in the structural editor, and writes each edit back into the document.

## Workspace layout

Each half is a frontend-neutral **model** and an editor **widget** that draws
it. The document's caret/selection model and AST→glyph mapping are
`flower-doc`; the data file's projection, cursor and lossless edits are
`flower-data`. Their widgets are `flower-doc-editor` and
`flower-data-editor`, on
Dioxus's native renderer, which draws HTML and CSS through
[the Blitz fork](https://github.com/diaryx-org/blitz) (branch `leaf`) with no
webview. Thin **apps** host the widget with a window or a terminal, a
clipboard and file I/O.

leaf's other frontends (Swift for iOS and the Mac, the web package and its wasm
binding, gpui for the desktop, Android, and the ratatui terminal editor) are
archived under [`archive/`](archive/README.md). They stay readable there as a
guide to what each platform needs; the tasks under `docs/tasks` say which of
those behaviours the Dioxus path does not have yet.

### `crates/` — Rust libraries

| crate | what it is |
| ----- | ---------- |
| [`flower-doc`](crates/flower-doc)       | the document model — a `twig::Editor` with a byte-offset caret + selection, and the WYSIWYG `VisualMap`. Glyphs carry a **toolkit-agnostic `Style`**; no UI dependency. Fenced code is syntax-highlighted here too (syntect, two-face's grammars — the set `plates` publishes with), as an eight-way `Token` per glyph that each frontend colours its own way; the `syntax` feature carries the grammars and can be left out. |
| [`flower-raster`](crates/flower-raster)   | the **shared CPU rasterization layer**: image decode (SVG included), path resolution and aspect-fit policy, and oversized-heading rasters with the caret and selection painted into the pixels — how a terminal heading stays editable when cells can't draw over a graphics-protocol image.                    |
| [`flower-doc-editor`](crates/flower-doc-editor) | the **embeddable widget on Dioxus's native renderer** (`dioxus-native`, Blitz on wgpu): the editor, its rows, input method, find bar and status line, drawn in the `Look` its host gives it and asking a `Host` for the clipboard, Save, the zoom and the context menu. A standalone workspace, since `dioxus-native` is an alpha. |
| [`flower-pages`](crates/flower-pages) | the **Reflow and Pages views** over that widget: the document set by Typst (twig-export) on a worker, drawn a picture a block, with the caret, the selection and presses on Typst's own glyphs, scrolled and zoomed; the built-in templates set in process. A host answers a `Host` (its editor, its area, a way to wake its UI) and may give a `Setup` (installed templates, where else to set them). |
| [`flower-math`](crates/flower-math)       | TeX to a **self-contained SVG picture** with baseline metrics, through RaTeX with KaTeX's outlines embedded — the one door every frontend draws a formula through, so the picture is byte-identical everywhere.                                                    |
| [`flower-mermaid`](crates/flower-mermaid) | Mermaid diagrams to SVG, through mmdr, for the editor to draw a fence with. |
| [`flower-session`](crates/flower-session) | the editors flower runs itself for a region of a document (a formula today), behind one interface: keys in, splices on the region's content out. |
| [`flower-math-edit`](crates/flower-math-edit) | a structural formula editor whose file format stays TeX (a spike), which `flower-session` runs. |
| [`flower-data`](crates/flower-data) | the data file's model, over `fig`: the page projection (one container a page, small groups inlined), the cursor, typed scalar edits, comments, undo and redo, a schema's vocabulary for a field, and a host's findings beside the rows they are about. No UI and no file system. [Its README](crates/flower-data/README.md) is the guide to this half: the keys, the tiers, the `Backend` trait. |
| [`flower-data-editor`](crates/flower-data-editor) | the **data file's widget on Dioxus's native renderer**: the pages as a settings screen out of unit's components — a switch for a boolean, chips for a short list, a menu for a field with a vocabulary, values typed in place. |
| [`flower-ratatui`](crates/flower-ratatui) | the data file's widget for a ratatui terminal, which provui's terminal frontend hosts. |
| [`flower-ffi`](crates/flower-ffi) | `flower-data` behind UniFFI, for the Swift package in [`packages/flower-swift`](packages/flower-swift) (`Package.swift` at the root is its manifest). |

### `apps/` — runnable frontends

| app | what it is |
|-----|------------|
| [`flower`](apps/flower) | **Flower**, in Rust from `main` up, for every platform Dioxus's native renderer reaches — the Mac's is `Flower.app` (`org.diaryx.flower`). A window per file. A data file's is `crates/flower-data-editor`'s pages. A document's is a Dioxus page: the toolbar, `crates/flower-doc-editor`'s editor, its frontmatter in the structural editor when asked, the context menu and Settings, their buttons, menus and dialogs [unit](https://github.com/diaryx-org/unit)'s. What a platform does its own way is `platform/`: on the Mac an `NSDocument` per window (Open, autosave, Versions, Rename, recents, Finder's Open With), the menu bar, the spelling checker, Look Up and the print panel, through objc2. Export as PDF and Print from the editor's own layout. What it does not do yet is the task "What the Mac app does not do yet". |
| [`flower-tui`](apps/flower-tui) | the terminal editor: the same `flower-doc-editor` editor, laid out by Blitz and painted into character cells by `blitz-tui`, with unit's menus and dialogs in their cell density. It replaces the archived `leaf-tui`; what it does not do yet is in [its proposal](docs/proposals/the-leaf-dioxus-editor-in-a-terminal.md) and the task "The terminal editor after leaf-tui". |

| [`flower-data-tui`](apps/flower-data-tui) | the terminal editor for a data file, on `flower-ratatui`: the binary `flower` that the tap's `flower` formula installs. |
| [`flower-web`](apps/flower-web) | the document editor in a browser tab (`cargo xtask web`). |

```sh
cargo run -p flower-app -- path/to/document.md    # or a data file
cargo run -p flower-tui -- path/to/document.md
cargo run -p flower-data-tui -- path/to/config.toml
```

The terminal and the browser open one half each still: a task says so.

Every crate and app is a member of one workspace, on Dioxus 0.8's alpha;
the root `Cargo.toml` patches in the Blitz fork, the Mac's winit backend
and wgpu's hardware layer for all of them.

The Mac app is a bundle put together around its binary, so it has a task in
the runner in [`xtask/`](xtask):

```sh
cargo xtask mac              # build Flower.app (apps/flower) and launch it, with a copy of the sample open
cargo xtask mac notes.md     # …with a document of your own
cargo xtask package          # a signed, notarised Flower.app in a .dmg, in target/package/
```

On Linux, a development build is installed for the user, with the icon and a
desktop entry, by a script; run it again to update:

```sh
scripts/install-linux.sh     # ~/.local/bin/flower-app, and Flower in the app launcher
```

`cargo xtask --help` lists every task and flag. `cargo xtask package` needs a
Developer ID identity in the keychain and notary credentials in the
environment; the head of [`xtask/src/package.rs`](xtask/src/package.rs) names
them, and `--no-notarize` does without the second.

The runner holds the tasks that leave Rust; the checks and releasing are
[`ship`](https://github.com/diaryx-org/ship). `ship ci` runs everything a
release has to pass — rustfmt, clippy with warnings denied, the workspace
tests, a `cargo check` of each crate alone in each feature shape a host
actually builds, and the web host and the widget linted for the browser. `ship <patch|minor|major|X.Y.Z>` runs those checks,
moves the version through every manifest and the lockfile, cuts the generated
region of [`docs/CHANGELOG.md`](docs/CHANGELOG.md) into a released section,
and commits and tags — locally. Pushing takes `--push`, and a pushed tag is a
release: [`mac-app.yml`](.github/workflows/mac-app.yml) attaches a signed,
notarised `Flower-<version>-aarch64.dmg` to the release and points the tap's
`flower-editor` cask at it (`brew install --cask diaryx-org/tap/flower-editor`),
[`homebrew.yml`](.github/workflows/homebrew.yml) builds the terminal data
editor and writes the tap's `flower` formula, and
[`pages.yml`](.github/workflows/pages.yml) deploys the browser build.
[docs/releasing.md](docs/releasing.md) is what a release does, step by step.
The crates are not on crates.io (leaf's last upload was 0.5.0, flower's
0.6.4, under the names they had then): provui and the
Diaryx app name them from git, `{ git = "https://github.com/diaryx-org/flower",
branch = "main" }`, and lock the commit they build.

`ship` is the org's release tool, which its other repositories cut releases
with too; what makes flower flower is [`.config/ship.figl`](.config/ship.figl) and
nothing else.

## The document editor

Sibling to [`bough`](https://github.com/diaryx-org/bough): **same backend, opposite model.** Where bough
moves a selection through the document's AST and edits the *tree*, flower's document editor gives you
an ordinary text **caret**, mouse, selection, and a formatting toolbar — and
turns every keystroke into one of twig's offset-addressed edits. The document
stays a live, round-trippable AST the whole time you type into it, so a Markdown
file and a Djot file are edited through the exact same operations.

Two views:

- **source** — the raw document with the caret in source bytes.
- **wysiwyg** — the markup *resolved*: headings coloured, `**bold**` as real
  bold, `==🔴 text==` as a red highlighter wash, a `` ```rust `` fence
  syntax-highlighted, the `#` / `**` / `` ` `` delimiters hidden. The caret
  still works because
  every rendered glyph is tied back to the source byte it came from, so cursor
  motion, clicks, and selection ride the *visible* text and step right over the
  hidden delimiters. Because it reads the AST, Markdown and Djot that parse alike
  render identically — the [`mdfried`](https://github.com/benjajaja/mdfried)
  idea, made editable.

Every action maps onto twig's editor surface:

| action | twig op |
|--------|---------|
| type / delete | `edit_range(start, end, text)` |
| re-anchor the caret after an edit | the returned `Change` |
| breadcrumb / cursor context | `ancestors_at(offset)` |
| click to place the caret | `node_at` + the flat `nodes()` snapshot |
| **bold / italic / code / mark** | `toggle_inline(range, kind)` |
| **heading / body** | `set_block(offset, kind)` |
