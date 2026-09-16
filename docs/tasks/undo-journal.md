---
title: Undo and redo in the model
description: Every edit reaches the backend through one `commit`, and nothing remembers what it replaced; flower should keep a journal of inverse edits so a value, a deleted key, a reorder or a comment edit can be undone and redone, and so a host composing flower with another editor can interleave both histories
status: open
created: 2026-09-15
updated: 2026-09-15
part_of: '[Tasks](/docs/tasks/tasks.md)'
---

# Undo and redo in the model

**Where this starts.** `Model::commit` is the only way an edit reaches a
backend: it applies one `EditOp`, refreshes the value tree, and sets the
status line. `EditOp` is a closed set — replace, insert key, delete key,
append item, remove item, move item, reorder keys, and the comment ops — and
`Backend::apply` is atomic, so the document is always in a state the parser
accepts. What is missing is memory. A typo committed into a value is fixed by
editing it again; a key deleted with `x` is gone, along with its leading
comment, and the only way back is to quit without saving. `leaf`, the sibling
editor, has had undo and redo since its first release, and a host that puts
the two side by side (provui's `DocumentSession`) can undo a keystroke in the
body and not a keystroke in the frontmatter, which is the kind of asymmetry
a user reads as a bug.

**What this adds.**

- **A journal in `Model`**, not in the backend. Before `commit` applies an
  op, it derives the op's *inverse* from the current value tree and pushes
  the pair. Every `EditOp` has one in the same vocabulary: a replace
  inverts to a replace with the old value; an insert to a delete; a delete
  to an insert followed by the reorder that restores its position; an
  append to a remove; a remove to an append and a move; a move to the
  reverse move; a reorder to a reorder with the old order; a comment edit
  to one with the old text. A backend gains undo without a line changing,
  which is what keeps `ProvBackend` in provui on the same footing as
  `FigBackend`.
- **`undo()` and `redo()` on `Model`**, each applying the stored op through
  the same `apply`, so a managed key that declined the edit declines the
  undo too and the status line says so. Undo places the cursor at the
  anchor the original commit was made from, so the row that changes is the
  one on screen. Redo is cleared by the next fresh commit.
- **A boundary hosts can see.** `Model::history_len()` and a monotonic
  `edit_seq()`, so a host that holds two editors can keep one ordered
  history of "body edit, metadata edit, body edit" and dispatch each undo
  to the editor whose turn it is. provui's session is the first caller and
  the reason the sequence number exists.
- **Keys and controls.** `u` and `U` in the widget, where `u` is unbound
  and reads as vi; an undo/redo pair in `FlowerModel` for the Swift view.
  Both surfaces come from the one command table, as the rest do.

**What it does not do.** Save is not a journal boundary — undo runs back
through a save, and the dirty flag is recomputed from the source rather than
from the journal's depth, so undoing to the saved text reads as clean.
Undoing a *delete* recreates the node through the backend's insert, which
serialises the value afresh: the key comes back where it was and with its
leading comment, but a quoting style or a number's spelling fig's editor
does not carry into a `Value` may come back normalised. A byte-exact undo
belongs in fig's editor, as a splice log, and would let this journal store
the bytes instead of the inverse op; that is a fig task if the normalisation
turns out to matter, and this task does not wait for it.

**Done when** any sequence of edits on a TOML, YAML, or fig document undone
to the start leaves the value tree equal to the opened one and the source
byte-identical wherever no node was deleted; redo replays to the same bytes
the edits produced; a declined edit on a managed key is a no-op in both
directions; and provui can interleave leaf's undo with flower's by the
sequence number alone.
