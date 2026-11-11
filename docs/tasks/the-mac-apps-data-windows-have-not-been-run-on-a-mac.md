---
title: The Mac app's data windows and frontmatter view have not been run on a Mac
part_of: '[Tasks](/docs/tasks/tasks.md)'
status: open
description: The data-file window and the frontmatter view were built and run on Linux; the Mac's NSDocument half is type-checked and linted for aarch64-apple-darwin and nothing more
created: 2026-11-11
---
# The Mac app's data windows and frontmatter view have not been run on a Mac

**What.** When `apps/flower` learned to open a data file (2026-11-11) the work
was done on Linux: the app was run there on a TOML file and a Markdown file
with frontmatter, edited, saved and undone. The Mac's side —
`platform/macos/document.rs` (`DATA_TYPES`, `data`, `take_source`, a parse
check in `readFromData:ofType:error:`), the data types in `Info.plist`, and
the menus' validation for a data window (`commands::status`) — compiles and
passes clippy for `aarch64-apple-darwin` with stand-in C tools, and has not
run.

**To check on a Mac.** `cargo xtask mac sample.toml`, then: the window opens
below the title bar's band and its bar is not under the traffic lights; an
edit sets the edited dot and autosaves; Revert and Versions put back a
parsed tree; a `.json` that does not parse is refused with AppKit's alert;
File ▸ Save As offers the file's own type; Edit ▸ Undo and Redo are enabled
by the tree's history and the document's commands are off; Finder's Open
With lists Flower for `.toml`, `.yaml`, `.json`, `.zon` and `.figl`. Then a
Markdown file with frontmatter: View ▸ Frontmatter is ticked while shown and
off for a document with none.

**Also unverified there.** The bundle is `org.diaryx.flower` with leaf's
icon (`resources/leaf.icon`, `CFBundleIconName` `leaf`), and no longer
sandboxed as the data editor's app was.
