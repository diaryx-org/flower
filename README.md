# flower

A structural TUI editor for config files, built on [`fig`](../fig).

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

- Open a file (format detected from extension), render its structure as an
  indented, type-colored tree.
- Navigate structurally: move between siblings, into children, out to parents;
  expand/collapse containers.
- Read it as a **settings menu** instead (`v`): one container per page, small
  all-scalar groups inlined, and a container short enough to fit shown in flow
  form (`push  {branches: [master]}`) rather than counted. Two panes when there's
  width and depth to use them — consecutive levels of one lineage, so the left is
  always the page the right came out of — and one when there isn't. Depth costs a
  page rather than a column, so a deeply nested document stays as legible as a
  shallow one.
- Sink the fields nobody types in (a recomputed hash, a relation another pane
  owns) below the ones they do, subtree and all, without hiding them.
- Edit a scalar in place (typed: `true`/`42`/`3.14`/`null`/text) — committed via
  `fig::Editor::replace_value`, so the splice is lossless and validated.
- Delete a mapping entry or sequence item.
- Save (writes fig's edited source back to disk).

Deliberately not here yet — see the roadmap.

## Keys

| Key | Tree view | Page view |
|-----|-----------|-----------|
| `j` / `k` (or ↓/↑) | next / previous row | next / previous item |
| `l` (or →) | expand container, or step into first child | open the container as a page |
| `h` (or ←) | collapse container, or step out to parent | back to the parent page |
| `Enter` / `Space` | toggle a container / edit a scalar | open a container / edit a scalar |
| `v` | switch to the page view | switch to the tree |
| `e` | edit the selected scalar | ← same |
| `x` | delete the selected entry or item | ← same |
| `s` | save to disk | ← same |
| `q` | quit | ← same |

`v` carries the cursor across, so the node you were on in one view is the node
you land on in the other. The keys that operate on a *node* mean the same thing
in both views — an edit is a path and a value, and neither view owns it.

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
| widget | [`crates/flower-ratatui`](crates/flower-ratatui) | a `draw(frame, &Model, header)` ratatui widget. |
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
driven by row index) remains for a custom renderer, but the packaged Swift
surface is the page view alone. The page methods address nodes by the dotted
path a row already carries rather than by index, because a page item need not be
a visible *row* at all. How much of the document one page holds is the host's
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
- **`crates/flower-ratatui`** — a `draw(frame, &Model, header)` widget. Depends
  on `flower-core` + `ratatui`.
- **`crates/flower-tui`** — the terminal app (binary `flower`): reads the file,
  runs the event loop, writes on save. Depends on both.

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
- A **prov backend** (`ProvBackend`, in the [`provui`](../provui) repo) drives
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
- **Comments**: show and edit leading/trailing comments (`fig::Editor` exposes
  `leading_comment`/`set_trailing_comment`/…).
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
  picker), keyboard navigation, insert/reorder, and comment display — the same
  roadmap the TUI has, in SwiftUI.
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

`flower-core` and `flower-ffi` are on crates.io — the binding crate because its
view projection is generic over the `Backend`, so an embedder with its own can
render a `Model` without reimplementing it. `flower-ratatui` and `flower-tui` are
`publish = false` and move with the same version number. See
[docs/releasing.md](docs/releasing.md) for how a release is cut and
[docs/CHANGELOG.md](docs/CHANGELOG.md) for what has changed.
