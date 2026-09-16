---
part_of: id:org/kv2bv2m
title: flower
contents:
- '[flower on diaryx.org](/www/index.md)'
- '[Audiences](/vocab/audiences.md)'
- '[Tasks](/docs/tasks/tasks.md)'
config: .config/prov.yaml
registry: registry.yaml
id: rkhvqqw
---
# flower

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
- Save (writes fig's edited source back to disk).

Deliberately not here yet — see the roadmap.

## Keys

| Key | What it does |
|-----|--------------|
| `j` / `k` (or ↓/↑) | next / previous item on the page |
| `l` (or →) | open the container as a page; on a scalar, edit it |
| `h` (or ← / `Esc`) | back to the page that listed the container you opened |
| `Enter` / `Space` | ← same as `l` |
| `e` | edit the selected scalar |
| `c` | edit the comment after the selected value (one line; empty removes it) |
| `C` | edit the comment block above the selected node (empty removes it) |
| `x` | delete the selected entry or item |
| `s` | save to disk |
| `q` | quit |

One projection, so one table. flower-core still offers the indented tree it
started as, and an embedder driving the model by row index still uses it — the
terminal doesn't, because depth there costs an indent column every row below
pays for, where a page spends it once on a breadcrumb.

In edit mode: type to change the value, `Enter` to commit, `Esc` to cancel.

## Usage

```bash
cargo run -- path/to/config.toml
```

Supported formats: JSON/JSONC/JSON5, YAML, TOML, ZON, and the `fig` dialect
(`.fig`/`.figl`) — anything fig's default feature set parses.

## Architecture

A Cargo workspace, split so the editing logic is frontend-neutral (mirroring
`leaf-core` / `leaf-ratatui` / `leaf-tui`):

```
fig (Zig) → fig-sys (FFI, libfig.a) → fig crate (Editor/Document/Value)
                                          │
                          crates/flower-core   (the model — no UI, no fs)
                    ┌────────────────────────┼─────────────────────────┐
      crates/flower-ratatui        crates/flower-ffi            crates/flower-tui
         (ratatui widget)        (UniFFI C-ABI binding)      (app: file I/O + loop)
                                          │
                              packages/flower-swift
                     (FlowerFFI + FlowerUI + FlowerPagesUI)
                                          │
                               apps/flower-editor
                             (macOS/iOS example app)
```

### The tiers

| Tier | Path | What it is |
|------|------|------------|
| core | [`crates/flower-core`](crates/flower-core) | the frontend-neutral model — the navigable `Row` tree + path-addressed lossless edits over fig. No UI, no fs. |
| widget | [`crates/flower-ratatui`](crates/flower-ratatui) | the ratatui widget: `draw(frame, &Model, header)` draws the page view, `handle_key(&mut Model, KeyEvent)` drives it and returns an `Outcome` naming what the host must do (quit, save) — the same division as `leaf-ratatui`. |
| binding | [`crates/flower-ffi`](crates/flower-ffi) | the **UniFFI C-ABI binding** — wraps the filesystem-free `Model` so a native Apple app can drive it. The native-Apple peer of the ratatui widget. |
| app | [`crates/flower-tui`](crates/flower-tui) | the terminal app (binary `flower`) — file I/O + event loop. |
| Swift SDK | [`packages/flower-swift`](packages/flower-swift) | the Swift Package (manifest at the repo root, so SwiftPM can resolve it by version). `FlowerPagesUI` is the page view (`FlowerPages`) written against protocols, with **no binding behind it**; `FlowerUI` is `FlowerModel` over the UniFFI `flower-ffi` binding, and the conformances that let the page view render its records. `import FlowerUI` re-exports both. |
| Swift app | [`apps/flower-editor`](apps/flower-editor) | the cross-platform (macOS + iOS) SwiftUI example, consuming `packages/flower-swift`. |

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
apps/flower-editor/bootstrap.sh           # generate the Swift binding + Xcode project
```

The editor runs today on macOS and on the iOS simulator: Xcode's build phase
compiles the `flower-ffi` staticlib for whichever slice it is building, so the
simulator gets one from source via Zig cross-compiling rather than from the
prebuilt macOS-arm64 lib fig-sys ships.

`scripts/build-xcframework.sh` has not caught up — it still assembles one slice
(`aarch64-apple-darwin`), so a *distributable* `FlowerFFI.xcframework` covering
macOS-x64 and a real device is the step that remains. Running from a checkout
does not need it.

- **`crates/flower-core`** — the frontend-neutral model. Depends only on `fig`
  and `std`.
  - `format.rs` — file extension → `fig::Format`.
  - `tree.rs` — flattens a `fig::Value` into navigable `Row`s, each carrying its
    `fig` path (a `Vec<Seg>` of `Key`/`Index`) — exactly what `fig::Editor` ops
    take — honoring a collapsed-set.
  - `page.rs` — the other projection: one container's children as a `Page`, with
    a container whose subtree fits the **inline budget** (`InlineBudget` — a row
    count and a rank depth, default: small and all-scalar) inlined into its
    parent's page as a titled group rather than given one of its own. Raised
    past the document's size, the root page *is* the whole document — the
    settings-list rendering, from the same projection. Same paths, so the same
    edits. A sequence's
    items render alike (a list where some rows are expanded and others collapsed
    reads as a fault), and are titled by whichever of their fields best names
    them — `title_keys` scores coverage, distinctness, and convention, so a
    workflow's steps list as `actions/checkout@v7` rather than as `[0]`. An
    embedder can **demote** top-level keys (`Model::set_demoted`) so the fields
    nothing hand-edits — a recomputed hash, a relation the sidebar owns — render
    below the ones a reader came for; the mark covers the whole subtree, so
    opening a demoted container stays inside the section.
  - `model.rs` — `Model`: owns the `fig::Editor` (source of truth), the derived
    `Value`/rows, selection, and the edit ops. Constructed from bytes; the
    embedder owns the file.
- **`crates/flower-ratatui`** — the widget: `draw(frame, &Model, header)` draws
  the page projection, and `handle_key(&mut Model, KeyEvent) -> Outcome` owns the
  key table for both modes — navigation and edits happen in the widget, while the
  keys it cannot answer for itself come back as `Outcome::Quit` / `Outcome::Save`
  for the host, which is the only party with a terminal and a file. A
  third-party TUI can therefore embed flower as a pane without reimplementing
  the app's event loop. Depends on `flower-core` + `ratatui`.
- **`crates/flower-tui`** — the terminal app (binary `flower`): reads the file,
  runs the event loop, forwards each key to the widget, and writes on
  `Outcome::Save`. Depends on both.

The read path is `fig::Document::to_value()` (a semantic `Value` tree); the write
path is `fig::Editor`'s path-addressed ops. After every edit the model re-derives
the tree from `Editor::source()`, so the editor's owned source is always
canonical.

### The commit-sink `Backend` trait

`flower-core::Model` is generic over a `Backend` — it never touches a concrete
editor. It builds path-addressed `EditOp`s, applies them through the backend,
and reads the tree back via `Backend::to_value`:

```rust
pub trait Backend {
    fn apply(&mut self, op: EditOp) -> Result<(), BackendError>;
    fn to_value(&self) -> Result<Value, BackendError>;   // metadata region, for rendering
    fn source(&self) -> Result<String, BackendError>;    // full bytes, for save
}
```

- `FigBackend` (in flower-core) drives a raw `fig::Editor` — a standalone config
  file.
- A **prov backend** (`ProvBackend`, in the [`provui`](https://github.com/diaryx-org/provui) repo) drives
  the *metadata region* of a prov document through prov's carrier-aware
  `MetaEditor`, leaving the prose body untouched. That composition (flower for
  metadata + leaf for the body, over one prov document) is proven by a headless
  test in provui.

flower-core stays config-generic and prov stays consumer-agnostic, so the
app-specific bridge lives in provui, not here — flower doesn't depend on prov.

## Roadmap

- **Value-editing affordances**: type-aware widgets (bool toggle, enum picker,
  number stepper) instead of one free-text field; today's edit coerces by
  literal shape, which a schema layer would fix.
- **Insert**: add keys / append sequence items (`fig` already exposes the ops).
- **Reorder / move**: `move_key`, `reorder_keys`, `move_item`.
- **Commented-out entries**: a `# port = 8080` is, to fig, the leading comment
  of the next entry (or the container's dangling run when it was last), and
  flower shows it as exactly that. Showing it as a *disabled entry* — a ghost
  row with a toggle — needs fig to read and write the dangling anchor and to
  comment a node out (and back in) at the byte level, since only fig has the
  spans to do that losslessly. Filed as fig's
  [`dangling-comments-and-comment-out-ops`](https://github.com/diaryx-org/fig/blob/main/docs/tasks/dangling-comments-and-comment-out-ops.md)
  and, on this side, [`disabled-entries`](docs/tasks/disabled-entries.md).
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
- **Stable identity for a sequence item**: a path addresses one by index, so
  reordering or deleting an earlier sibling silently re-points every id after it.
  Core re-finds the *cursor* across an edit, but a breadcrumb and a navigation
  stack hold ids, so a screen you pushed can come to name a different item. A
  per-item identity (fig has none today) would fix both, and the tree's row ids
  with it.
- **Native frontend affordances** (`FlowerUI`): today it edits scalars in one
  inline text field. Next: type-aware widgets (bool toggle, number stepper, enum
  picker), keyboard navigation, insert/reorder, and comment *editing* — the
  rows show comments, and `FlowerModel` can set them, but no control opens one
  yet. The same roadmap the TUI has, in SwiftUI.
- **The rest of the Apple slices**: cross-compile `fig` via Zig for macOS-x64,
  iOS, and the simulator so `scripts/build-xcframework.sh` produces a full
  `FlowerFFI.xcframework`.

## Development

CI is a program, not a YAML file: `cargo xtask ci` runs every job the workflow
runs, in the same order, and `cargo xtask <job>` runs one.

| Job | What it runs |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `clippy` | `cargo clippy --workspace --all-targets -- -D warnings` |
| `test` | `cargo test --workspace` |
| `package-isolation` | each crate built alone, so workspace feature unification can't hide a crate that fails on its own |
| `msrv` | a build on `workspace.package.rust-version` (1.88) |

The Swift half needs macOS and Xcode, so it is run by hand:
`scripts/check-swift.sh` type-checks both Swift targets, `scripts/test-swift.sh`
runs their tests, and `scripts/build-xcframework.sh` produces the distributable
framework. The check compiles `FlowerPagesUI` first and alone, with no binding on
the search path: the only way to keep a target FFI-free is to compile it
somewhere an FFI import would not resolve.

`flower-core`, `flower-ratatui`, and `flower-ffi` are on crates.io — the binding
crate because its view projection is generic over the `Backend`, so an embedder
with its own can render a `Model` without reimplementing it. `flower-tui` is
`publish = false` and moves with the same version number. See
[docs/releasing.md](docs/releasing.md) for how a release is cut and
[docs/CHANGELOG.md](docs/CHANGELOG.md) for what has changed.
