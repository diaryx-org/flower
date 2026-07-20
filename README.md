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
- Edit a scalar in place (typed: `true`/`42`/`3.14`/`null`/text) — committed via
  `fig::Editor::replace_value`, so the splice is lossless and validated.
- Delete a mapping entry or sequence item.
- Save (writes fig's edited source back to disk).

Deliberately not here yet — see the roadmap.

## Keys

| Key | Action |
|-----|--------|
| `j` / `k` (or ↓/↑) | next / previous row |
| `l` (or →) | expand container, or step into first child |
| `h` (or ←) | collapse container, or step out to parent |
| `Enter` / `Space` | toggle a container / edit a scalar |
| `e` | edit the selected scalar |
| `x` | delete the selected entry or item |
| `s` | save to disk |
| `q` | quit |

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
                             │                     │
              crates/flower-ratatui (widget)   crates/flower-tui (app: file I/O + event loop)
```

- **`crates/flower-core`** — the frontend-neutral model. Depends only on `fig`
  and `std`.
  - `format.rs` — file extension → `fig::Format`.
  - `tree.rs` — flattens a `fig::Value` into navigable `Row`s, each carrying its
    `fig` path (a `Vec<Seg>` of `Key`/`Index`) — exactly what `fig::Editor` ops
    take — honoring a collapsed-set.
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

- `FigBackend` (shipped) drives a raw `fig::Editor` — a standalone config file.
- A future **prov backend** will apply the same ops through prov's frontmatter
  editor, so the GUI gets inverse-link / fixity / journaling maintenance for
  free without `Model` knowing about them. That is the seam a `prov` GUI plugs
  into. `to_value` returning the value tree (not just source) is deliberate: an
  embed/prov backend renders the *metadata region*, not the whole host file.

## Roadmap

- **Value-editing affordances**: type-aware widgets (bool toggle, enum picker,
  number stepper) instead of one free-text field; today's edit coerces by
  literal shape, which a schema layer would fix.
- **Insert**: add keys / append sequence items (`fig` already exposes the ops).
- **Reorder / move**: `move_key`, `reorder_keys`, `move_item`.
- **Comments**: show and edit leading/trailing comments (`fig::Editor` exposes
  `leading_comment`/`set_trailing_comment`/…).
- **Schema layer**: the big one — fig has none, so a "what keys/values are valid
  here" layer is ours to add; unlocks completion, typed widgets, validation.
- **Frontend-neutral core**: if a GUI (gpui) frontend follows, split the tree /
  selection / edit logic out of the ratatui-specific bits, mirroring
  `leaf-core`.
