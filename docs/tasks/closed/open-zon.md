---
title: Open a ZON file
description: flower-core opens ZON since fig 7 (it depends on fig-zon), so flower-tui and the FFI accept `.zon`, and the Mac app declares the type
status: done
created: 2026-09-30
updated: 2026-11-03
part_of: '[Closed tasks](/docs/tasks/closed/closed.md)'
---

# Open a ZON file

**Status: done.** `apps/flower-dioxus/Info.plist` imports `org.ziglang.zon` and
lists it among the app's document types, as it does `org.diaryx.fig`; the Open
panel already offered `zon`. A test in `apps/flower-dioxus/src/file.rs` holds
the two together. Checked on Linux, against the plist alone: it has not been
tried in the Finder.

**Repro.** In the Mac app, a `.zon` file cannot be opened: the app declares no
document type for it.

**The Rust half is done.** Through fig 5, `zon` was a non-default fig feature,
and turning it on took fig-sys off its prebuilt archive and onto a Zig source
build whose macOS deployment target was the build machine's. fig 7 is pure
Rust and every language is a crate of its own, so flower-core depends on
`fig-zon` and `flower_core::format::from_extension("zon")` (which flower-tui and
flower-ffi's `parse_format` both go through) opens one:

```sh
printf '.{\n    .name = .flower,\n}\n' > build.zig.zon
cargo run -p flower-tui -- build.zig.zon
```

**Done when** the app declares the type. `.zon` has no system UTI, so the app
needs an imported `org.ziglang.zon` in `apps/flower-dioxus/Info.plist`, and
`zon` among the extensions its Open panel offers (`apps/flower-dioxus/src/file.rs`).
