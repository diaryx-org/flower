---
title: Open a ZON file
description: flower-tui names `zon` among the extensions it accepts, and fig can parse ZON, but the workspace builds fig without its `zon` feature, so every `.zon` is refused as an unsupported format; the Mac app leaves the type out for the same reason
status: open
created: 2026-09-30
updated: 2026-09-30
blocked_by: "[fig: source-build-deployment-target](https://github.com/diaryx-org/fig/blob/main/docs/tasks/source-build-deployment-target.md)"
part_of: '[Tasks](/docs/tasks/tasks.md)'
---

# Open a ZON file

**Repro.**

```sh
printf '.{\n    .name = .flower,\n}\n' > build.zig.zon
cargo run -p flower-tui -- build.zig.zon    # Error: unsupported format
```

`parse_format` in flower-ffi, and the TUI's extension match, both map `zon`
onto `fig::Format::Zon`. The format is compiled out because the workspace
depends on `fig = "5"` with its default features, and `zon` is not one of them.

**Why not just turn it on.** Any non-default feature takes fig-sys off its
prebuilt archive and onto a Zig source build. On a Mac, that build targets the
build machine's macOS (the blocking task), so the app and the Homebrew
binaries would stop launching on anything older than the machine that built
them. Checked on 2026-09-30: with `zon` on, `libfig_zcu.o` came out at
`minos 27.0`, against `13.0` for the prebuilt archive.

**Done when** `fig` is depended on with `features = ["zon"]` and the archive
it links still says the deployment target, and the app declares the type
again. `.zon` has no system UTI, so the app needs an imported
`org.ziglang.zon` in `apps/flower-editor/project.yml`, a `ZONFormat` whose
empty document is `.{}\n` (an empty file does not parse), and its own
`DocumentGroup`.
