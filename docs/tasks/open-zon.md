---
title: Open a ZON file
description: flower-core opens ZON since fig 7 (it depends on fig-zon), so flower-tui and the FFI accept `.zon`; the Mac app still leaves the type out
status: open
created: 2026-09-30
updated: 2026-10-15
part_of: '[Tasks](/docs/tasks/tasks.md)'
---

# Open a ZON file

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
needs an imported `org.ziglang.zon` in `apps/flower-editor/project.yml`, a
`ZONFormat` whose empty document is `.{}\n` (an empty file does not parse), and
its own `DocumentGroup`.
