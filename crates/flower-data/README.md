# flower-data

The structural editor for data files: the model, and what is built on it.
This is flower's README from before leaf's document editor joined it, kept
here as the guide to the data half: the model (`flower-data`), its terminal
widget and app (`flower-ratatui`, `apps/flower-data-tui`, whose binary is
`flower`), its UniFFI binding (`flower-ffi`) and the Dioxus widget
(`flower-data-editor`) that [Flower the app](../../apps/flower) shows a data
file and a document's frontmatter in. Paths are from the repository's root.

A structural TUI editor for config files, built on [`fig`](https://github.com/diaryx-org/fig).

Where a text editor edits characters, **flower edits the tree.** You navigate the
parsed config structurally — into mappings, along sequences, down to scalars —
and edit one *value* at a time. Every change is a path-addressed, lossless
splice through fig's editor: the bytes you didn't touch (comments, key order,
blank lines, quoting) stay byte-for-byte identical, and the document is only
ever committed in a valid state.

flower is to `fig` what `bough` is to `twig`: the structural editor over the
lossless AST. (`leaf` is the *rich-text* sibling — the right model for permissive
document formats, the wrong one for strict config grammars, where free-text
editing spends most of its keystrokes in states the parser refuses to hold.)

## Status

Early prototype. Working today:

- Open a file (format detected from extension) and read it as a **settings
  menu**: one container per page, small all-scalar groups inlined, and a
  container short enough to fit shown in flow form (`push  {branches: [main]}`)
  rather than counted. Two panes when there's width and depth to use them —
  consecutive levels of one lineage, so the left is always the page the right
  came out of — and one when there isn't. Depth costs a page rather than a
  column, so a deeply nested document stays as legible as a shallow one.
- Navigate structurally: along a page's items, into a container, back out to the
  page that listed it.
- Sink the fields nobody types in (a recomputed hash, a relation another pane
  owns) below the ones they do, subtree and all, without hiding them.
- Edit a scalar in place (typed: `true`/`42`/`3.14`/`null`/text) — committed via
  `fig::Editor::replace_value`, so the splice is lossless and validated.
- Read and edit the **comments** on a node: the block above it is drawn on the
  row above, dimmed, and the one after its value sits after the value, the way
  both read in the file. Either is edited in the same footer a value is, and
  emptied to remove it. A comment is the document's own note on a field — the
  closest thing an undescribed config has to help text — and fig anchors it to
  the node, so it moves with a reorder and goes with a delete.
- Delete a mapping entry or sequence item.
- **Undo and redo** every edit, including a delete and its comment: the model
  keeps a journal of inverse ops, so `u` walks back through a save as readily
  as through a keystroke and undoing to the saved text reads as clean again. A
  host pairing flower with another editor interleaves the two histories by
  `Model::edit_seq`.
- Draw the **host's findings** beside the rows they are about: a workspace
  check (a link that resolves to nothing, an id two documents share) is a fact
  flower cannot compute, so the embedder hands it over — `Model::set_annotations`
  — and every row that names one carries a marker, with its message in the
  status line.
- Save (writes fig's edited source back to disk).

Deliberately not here yet — see the roadmap.

## Keys

| Key | What it does |
|-----|--------------|
| `j` / `k` (or ↓/↑) | next / previous item on the page |
| `l` (or →) | open the container as a page; on a scalar, edit it |
| `h` (or ← / `Esc`) | back to the page that listed the container you opened |
| `Enter` / `Space` | ← same as `l` |
| `e` | pick a value for the selected scalar where it has a vocabulary, else edit it |
| `E` | edit the selected scalar as free text, vocabulary or not |
| `c` | edit the comment after the selected value (one line; empty removes it) |
| `C` | edit the comment block above the selected node (empty removes it) |
| `x` | delete the selected entry or item |
| `u` / `U` | undo / redo the last edit, wherever it was made |
| `s` | save to disk |
| `q` | quit |

One projection, so one table. flower-data still offers the indented tree it
started as, and an embedder driving the model by row index still uses it — the
terminal doesn't, because depth there costs an indent column every row below
pays for, where a page spends it once on a breadcrumb.

In edit mode: type to change the value, `Enter` to commit, `Esc` to cancel.

In the picker: type to filter the list (a case-insensitive substring of the
label), `j`/`k` or ↑/↓ to walk it, `Enter` to choose, `Esc` to cancel. What is
offered is the schema's vocabulary for the field — retired terms included, and
flagged — or, for a link field, whatever the backend could enumerate
(`Backend::candidates`); a field with neither opens the text line instead, so
one key does both.

## Usage

```bash
cargo run -- path/to/config.toml
```

Supported formats: JSON/JSONC/JSON5, YAML, TOML, ZON, and fig's `figl` dialect
(`.figl`, or `.fig`) — the fig language crates flower depends on.

## Architecture

A Cargo workspace, split so the editing logic is frontend-neutral (mirroring
`leaf-core` / `leaf-ratatui` / `leaf-tui`):

```
fig crate (Editor/Document/Value) + fig-json/-yaml/-toml/-zon/-figl
                                          │
                          crates/flower-data   (the model — no UI, no fs)
                    ┌────────────────────────┼─────────────────────────┐
      crates/flower-ratatui        crates/flower-ffi            apps/flower-data-tui
         (ratatui widget)        (UniFFI C-ABI binding)      (app: file I/O + loop)
                                          │
                              packages/flower-swift
                     (FlowerFFI + FlowerUI + FlowerPagesUI)

            crates/flower-data ── crates/flower-data-editor (Dioxus widget, on unit)
                                          │
                               apps/flower
                            (Flower, the Mac app)
```

### The tiers

| Tier | Path | What it is |
|------|------|------------|
| core | [`crates/flower-data`](../flower-data) | the frontend-neutral model — the navigable `Row` tree + path-addressed lossless edits over fig. No UI, no fs. |
| widget | [`crates/flower-ratatui`](../flower-ratatui) | the ratatui widget: `draw(frame, &Model, header)` draws the page view, `handle_key(&mut Model, KeyEvent)` drives it and returns an `Outcome` naming what the host must do (quit, save) — the same division as `leaf-ratatui`. |
| widget | [`crates/flower-data-editor`](../flower-data-editor) | the **Dioxus-native widget** (`dioxus-native` 0.8 alpha, Blitz on wgpu, no webview): the page view as a settings screen out of [unit](https://github.com/diaryx-org/unit)'s components — two panes, a section per group, chips, switches, menus, values typed in place, the picker in a modal, a context menu per row — with the terminal's keys and a window's (⌘Z, ⌫, ⌥↑/⌥↓), asking a `Host` for Save and the clipboard. A workspace of its own, as leaf's `crates/leaf-dioxus` is. |
| app | [`apps/flower`](../../apps/flower) | **Flower**, the app (`Flower.app` on the Mac): a data file opens in a window of its own, in the Dioxus widget, beside the documents the same app opens under `flower-doc-editor`. Its README says how it is built and packaged. |
| binding | [`crates/flower-ffi`](../flower-ffi) | the **UniFFI C-ABI binding** — wraps the filesystem-free `Model` so a native Apple app can drive it. The native-Apple peer of the ratatui widget. |
| app | [`apps/flower-data-tui`](../../apps/flower-data-tui) | the terminal app (binary `flower`) — file I/O + event loop. |
| Swift SDK | [`packages/flower-swift`](../../packages/flower-swift) | the Swift Package (manifest at the repo root, so SwiftPM can resolve it by version). `FlowerPagesUI` is the page view (`FlowerPages`) written against protocols, with **no binding behind it**; `FlowerUI` is `FlowerModel` over the UniFFI `flower-ffi` binding, and the conformances that let the page view render its records. `import FlowerUI` re-exports both. A host app of its own links it; flower's own Mac app is the Dioxus one. |

The Swift frontend keeps the same contract as the TUI: **core owns the model**
(the projection, selection, and every lossless edit), the frontend only renders
the frame and forwards navigation / edit intents. Every page call across the FFI
returns a `PagesView` — the page you are on, the page it came out of, and the
page the cursor would open, plus dirty and status — one crossing that both
mutates and repaints, so a two-pane host repaints whole from any edit.

Both projections cross the FFI: the tree's `DocView` (the flat visible-row list,
driven by row index) remains for a custom renderer, but every surface this repo
packages — the ratatui widget, the Swift `FlowerPagesUI` — is the page view
alone. The page methods address nodes by the dotted path a row already carries
rather than by index, because a page item need not be a visible *row* at all.
How much of the document one page holds is the host's
`setInlineBudget(rows:depth:)` — at the default, small all-scalar groups inline
and everything else drills; raised past the document's size, the root page is
the whole document, which is how the old settings-list surface was absorbed.

`FlowerPages` draws that frame two ways. Wide, it is the same sliding pair of
panes the TUI draws, moving along the trail in the direction you went. Narrow —
a small window, a phone — one column pushed and popped *is* a `NavigationStack`,
so it gets one, along with the OS's push animation, its back button, and the iOS
swipe-back gesture. A stack asks for the screen at an arbitrary path element
rather than being told, so `pageAt(id:)` builds a page without navigating to it;
the stack's path is a mirror of the model's focus, re-derived whenever either
side moves.

The two-pane layout is deliberately *not* a `NavigationSplitView`, whose sidebar
is fixed: it would put the root's list beside a page five levels away, which is
the arrangement the sliding window replaced.

```sh
cargo run -- path/to/config.toml          # the TUI
cargo xtask mac [path/to/config.toml]     # build and launch Flower.app, the Mac app
cargo xtask package                       # a signed, notarised Flower.app in a .dmg, in target/package/
```

### The commit-sink `Backend` trait

`flower-data::Model` is generic over a `Backend` — it never touches a concrete
editor. It builds path-addressed `EditOp`s, applies them through the backend,
and reads the tree back via `Backend::to_value`:

```rust
pub trait Backend {
    fn apply(&mut self, op: EditOp) -> Result<(), BackendError>;
    fn to_value(&self) -> Result<Value, BackendError>;   // metadata region, for rendering
    fn source(&self) -> Result<String, BackendError>;    // full bytes, for save
}
```

- `FigBackend` (in flower-data) drives a raw `fig::Editor` — a standalone config
  file.
- A **prov backend** (`ProvBackend`, in the [`provui`](https://github.com/diaryx-org/provui) repo) drives
  the *metadata region* of a prov document through prov's carrier-aware
  `MetaEditor`, leaving the prose body untouched. That composition (flower for
  metadata + leaf for the body, over one prov document) is proven by a headless
  test in provui.

flower-data stays config-generic and prov stays consumer-agnostic, so the
app-specific bridge lives in provui, not here — flower doesn't depend on prov.

## Roadmap

- **Value-editing affordances**: the **enum picker** has landed — `e` on a field
  with a vocabulary opens a filtered list, in the TUI and as a menu in the Swift
  rows — and a reference field's candidates arrive through
  `Backend::candidates`, the injection point a single-document core needs for a
  list of *other* documents. A bool toggle and a number stepper are still one
  free-text field in the TUI.
- **Insert**: add keys / append sequence items (`fig` already exposes the ops).
- **Reorder / move**: `move_key`, `reorder_keys`, `move_item`.
- **Commented-out entries**: a `# port = 8080` is, to fig, the leading comment
  of the next entry (or the container's dangling run when it was last), and
  flower shows it as exactly that. Showing it as a *disabled entry* — a ghost
  row with a toggle — needs fig to read and write the dangling anchor and to
  comment a node out (and back in) at the byte level, since only fig has the
  spans to do that losslessly. Filed as fig's
  [`dangling-comments-and-comment-out-ops`](https://github.com/diaryx-org/fig/blob/main/docs/tasks/closed/dangling-comments-and-comment-out-ops.md)
  and, on this side, [`disabled-entries`](../../docs/tasks/disabled-entries.md).
- **Comment as help text**: the Swift page view already lets a schema
  description win over the leading comment for the sentence under a name; the
  TUI shows the comment only. A schema description for the TUI is part of the
  schema layer below.
- **Schema layer**: the big one — fig has none, so a "what keys/values are valid
  here" layer is ours to add; unlocks completion, typed widgets, validation. It
  also supersedes the page view's structural guesses — inline-vs-drill, and which
  field titles a sequence item — with declared group titles and ordering. The
  same renderer, curated. The "advanced" rank is the piece that has landed: it is
  a set of keys the embedder names, and a schema would be where a document
  declares its own instead.
- **Stable identity for a sequence item**: half done. A path still addresses an
  item by index, but the model now records what the item *was* — the backend's
  own key (`Backend::item_key`, which a workspace-aware backend answers with a
  link target or a record id) or, failing that, the title the row already shows
  it by — and re-points the open page and the remembered cursors onto it after
  every edit, undo and redo. A reorder or the deletion of an earlier sibling no
  longer turns a page you opened into a different item. What remains is the ids
  a *host* holds: a navigation stack's path is still indices, and the tree's row
  ids with it, so a host that wants to survive a reorder asks `Model::item_key`
  and re-resolves as the model does.
- **Native frontend affordances** (`FlowerUI`): today it edits scalars in one
  inline text field. Next: type-aware widgets (bool toggle, number stepper, enum
  picker), keyboard navigation, insert/reorder, and comment *editing* — the
  rows show comments, and `FlowerModel` can set them, but no control opens one
  yet. The same roadmap the TUI has, in SwiftUI.
- **The rest of the Apple slices**: build `flower-ffi` for macOS-x64, iOS, and
  the simulator so `scripts/build-xcframework.sh` produces a full
  `FlowerFFI.xcframework`.

## Development

CI is the `job` list in `.config/ship.figl`, not a YAML file: `ship ci` runs
every job the workflow runs, in the same order, and `ship ci <job>` runs one.

| Job | What it runs |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `clippy` | `cargo clippy --workspace --all-targets -- -D warnings` |
| `test` | `cargo test --workspace` |
| `package-isolation` | each crate built alone, so workspace feature unification can't hide a crate that fails on its own |
| `bindings` | the committed UniFFI Swift binding regenerated from `crates/flower-ffi` and diffed |
| `msrv` | a build on `workspace.package.rust-version` (1.88) |

The Swift half needs macOS and Xcode, so it is run by hand:
`scripts/check-swift.sh` type-checks both Swift targets, `scripts/test-swift.sh`
runs their tests, and `scripts/build-xcframework.sh` produces the distributable
framework. The check compiles `FlowerPagesUI` first and alone, with no binding on
the search path: the only way to keep a target FFI-free is to compile it
somewhere an FFI import would not resolve.

`flower-data`, `flower-ratatui`, and `flower-ffi` are libraries for other
repositories to use — the binding crate because its view projection is generic
over the `Backend`, so an embedder with its own can render a `Model` without
reimplementing it. None of them is on crates.io (0.6.4 was the last upload): a
consumer names them from git, `{ git = "https://github.com/diaryx-org/flower",
branch = "main" }`, and locks the commit it builds. `flower-data-tui` moves with the
same version number. See
[docs/releasing.md](../../docs/releasing.md) for how a release is cut and
[docs/CHANGELOG.md](../../docs/CHANGELOG.md) for what has changed.
