---
title: Show a commented-out entry as a disabled row
description: A `# port = 8080` reads today as the leading comment of whatever entry follows it; flower should recognise it as an entry that is switched off, list it as a ghost row, and offer to switch it on — and switch a live entry off the same way
status: open
created: 2026-09-07
updated: 2026-09-07
blocked_by: "[fig: dangling-comments-and-comment-out-ops](https://github.com/diaryx-org/fig/blob/main/docs/tasks/dangling-comments-and-comment-out-ops.md)"
part_of: '[Tasks](/docs/tasks/tasks.md)'
---

# Show a commented-out entry as a disabled row

**Where this starts.** A page item carries the comment block above it and the
comment after its value, and both are edited in the footer (`C` / `c`). A commented-out line is therefore already *shown*
— as the block above the next entry, or not at all when it was the last
entry of its container, because fig's editor does not read the dangling run.

**What this adds.**

- **Detection**, in flower-core, when a page is annotated: parse each leading
  block (and, once readable, each dangling run) as a fragment in the
  document's format. A fragment that yields key-shaped entries whose keys are
  *absent* from the container is a run of disabled entries. One that repeats a
  present key is the "alternative value" pattern (`port = 8080` over
  `# port = 9090`) and stays a note; anything else is prose. The detection is
  conservative on purpose: a note that happens to parse costs a wrong toggle,
  a missed entry costs nothing.
- **A ghost row** per disabled entry: a new `ItemKind::Disabled` on
  `PageItem`, with the parsed label and preview, drawn dimmed in the widget
  and greyed in the Swift view, listed where the line sits in the file. It
  carries the anchor (which node's leading block, or which container's
  dangling run, and which lines of it) rather than a path, since it has none.
- **Toggling**: a key (and a Swift control) that enables a ghost row through
  fig's `uncomment*`, and disables a live entry through `commentOut`. Both
  are one `EditOp` each, lowered by `FigBackend`; a backend without them
  keeps the default, which is to offer neither.

**Why it waits.** Enabling from the middle of a block can be done today —
delete the block, re-add the rest — but re-serialising loses the entry's own
spelling, and a last entry has nowhere to be disabled *to*. Both are fixed by
the fig task linked above: the dangling anchor, and `commentOut` /
`uncommentLeading` / `uncommentDangling` as byte-level splices with a
parse-or-roll-back guarantee. This task picks up once a fig release ships
them and the `fig` pin here is bumped.

**Done when** a TOML, YAML, or fig document with a commented-out entry
lists it as a ghost row on the right page, toggling it on yields the entry
byte-for-byte as it was written, toggling a live entry off and on again is
byte-identical, and a leading block that is prose is still shown as a comment
and never offered as a toggle.
