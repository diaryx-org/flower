---
title: The terminal and the browser open one half each
part_of: '[Tasks](/docs/tasks/tasks.md)'
status: open
description: Flower the app opens a document or a data file; apps/flower-tui and apps/flower-web open documents only, and the terminal's data editor is a second binary on ratatui
created: 2026-11-11
---
# The terminal and the browser open one half each

**What.** `apps/flower` reads a file's extension and gives its window the
document editor or the structural one (`apps/flower/src/data.rs`), and shows
a document's frontmatter in the structural one (`front.rs`). The other two
hosts do neither: `apps/flower-tui` (binary `flower-dtui`) and
`apps/flower-web` host `flower-doc-editor` alone. A data file in a terminal
is `apps/flower-data-tui`, on `flower-ratatui`, whose binary is `flower` and
is what the tap's `flower` formula installs.

**Why it is deferred.** `flower-data-editor` asks dioxus-native for the
classic `vello` renderer and has not been laid out by `blitz-tui` or built
for `wasm32-unknown-unknown`; neither is known to work.

**Done when.** One terminal binary named `flower` opens either kind of file
through the Dioxus widgets, the browser build does too, and
`apps/flower-data-tui` is archived or kept on purpose, with `homebrew.yml`
pointing at whichever binary is `flower`. The task "Ship the Dioxus terminal
editor as leaf" is the document half of the same question.
