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

```
fig (Zig)  →  fig-sys (FFI, libfig.a)  →  fig crate (Editor/Document/Value)
           →  flower::app::App  →  flower::tree (Value → navigable Rows)  →  ratatui UI
```

- `src/format.rs` — file extension → `fig::Format`.
- `src/tree.rs` — flattens a `fig::Value` into a list of navigable `Row`s, each
  carrying its `fig` path (a `Vec<Seg>` of `Key`/`Index`), honoring a
  collapsed-set. This path is exactly what `fig::Editor` ops take.
- `src/app.rs` — editor state: owns the `fig::Editor` (source of truth),
  the derived `Value`/rows, selection, and the edit ops.
- `src/ui.rs` — ratatui rendering (header / tree / footer + edit line).

The read path is `fig::Document::to_value()` (a semantic `Value` tree); the write
path is `fig::Editor`'s path-addressed ops. After every edit we re-derive the
tree from `Editor::source()`, so the editor's owned source is always canonical.

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
