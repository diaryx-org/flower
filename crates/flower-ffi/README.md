# flower-ffi

The C-ABI / UniFFI **Rust binding** for flower: it wraps the filesystem-free
`flower-core` `Model` behind UniFFI so a native Apple app can drive the
structural config tree — navigate mappings/sequences/scalars and commit
path-addressed, lossless edits through fig.

This crate is only the Rust binding (`src/lib.rs` + the `uniffi-bindgen` bin).
The Swift side built on top of it lives elsewhere:

| Piece | Location | What it is |
|-------|----------|------------|
| Swift SDK | [`packages/flower-swift`](../../packages/flower-swift) | `Sources/FlowerUI` (the SwiftUI tree editor) + the committed `uniffi-generated/` Swift, exposed by the `Package.swift` at the repo root. The importable Swift package. |
| Demo app | [`apps/flower-editor`](../../apps/flower-editor) | The runnable example (`bootstrap.sh`, xcodegen `project.yml`). |

## The contract (same as every flower/leaf frontend)

- **Core owns the model.** The tree, the collapsed set, selection, and every
  path-addressed edit live in `flower-core`; the edit is spliced losslessly
  through `fig::Editor`, so comments, key order, and formatting survive.
- **The boundary is *visible rows*.** Every call returns a `DocView`: the flat
  list of currently-visible `RowView`s (core already honours the collapsed set)
  plus selection, dirty, and status — one crossing both edits and repaints.
- **Core owns the model; Swift owns the widgets.** Swift drives the model by
  **row index** (the coordinate a `List` selection speaks) and picks the
  affordance — a disclosure row, an inline text field, a type-aware widget.
- **No filesystem.** The host reads the file and persists `source()` its own way,
  then calls `markSaved()`.

## Build

The Swift bindings are (re)generated from this crate by
`scripts/gen-bindings.sh` into `packages/flower-swift/uniffi-generated/`, which
is **committed** — the Swift package is consumed by version from a bare git
checkout, so the binding must build as-is. CI's `bindings` job diffs the
committed binding against this crate on every push; regenerate and commit after
changing the FFI surface.

```sh
# Regenerate the binding after changing this crate's public FFI surface:
scripts/gen-bindings.sh
# What CI runs — fails if the committed binding is stale:
scripts/gen-bindings.sh --check
```

## The one `unsafe`

`fig::Editor` holds a `NonNull` (hence `!Send`), but a UniFFI object is a
thread-safe `Arc`, so the `Model` lives behind a `Mutex` and the guarded newtype
carries one `unsafe impl Send`: every access is serialized through that mutex and
the handle has no thread-affinity. Drive it from the main thread. See the safety
comment in `src/lib.rs`.
