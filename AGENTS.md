---
title: AGENTS
description: Rules for AI agents working in this repository
part_of: '[flower](/README.md)'
---
# flower

Rules for this repository alone; the org's `AGENTS.md` has the rest.

## Where it lives

The repository is on Tangled, https://tangled.org/adammharris.com/flower:
branches, pull requests and issues are there. `github.com/diaryx-org/flower`
is a mirror of it, which other repos' git dependencies still name, and where a
pushed tag's release workflows (`.github/workflows/`) run. A push of `main`
or a tag to Tangled is carried there by `.tangled/workflows/mirror.yml`.
Nothing is merged on GitHub, and nothing else pushes to it.

The gate's jobs run on Tangled's spindle for a push to `main` and for a pull
request: a file per job in `.tangled/workflows/`, each running `ship ci <id>`.
`tg` is the command line for Tangled, as `gh` is for GitHub
(`tg pipeline status`, `tg pipeline logs`, `tg pr create`).

## The gate

`ship ci` is the gate: rustfmt, clippy with warnings denied, the workspace's
tests, each crate checked alone (`cargo xtask package-isolation`), the Swift
binding against `crates/flower-ffi`, and the web host linted for
`wasm32-unknown-unknown`. `ship ci <id>` runs one job, `ship ci --list` names
them, and `.config/ship.figl` is where they are written.

## Building with corgi

**flower builds under corgi where corgi is installed** (`corgi.toml`,
https://corgi.build): one build store for every checkout on the machine, the
one unit and diaryx build in, so Blitz, Stylo and wgpu are built once for all
three. The gate's lanes call `scripts/cargo.sh`, which runs corgi where it is
on PATH and cargo where it is not, as on a spindle. Formatting,
`cargo xtask` itself and `scripts/gen-bindings.sh` are cargo's everywhere.
`FLOWER_CARGO=1` makes the script use cargo.

- Iterate under corgi too: `corgi test -p flower-app data`,
  `corgi clippy --workspace --all-targets -- -D warnings`. Its packages
  resolve together but for the browser's build, which has two `[roots]`:
  `corgi clippy --root web -p flower-web --target wasm32-unknown-unknown`,
  and `--root web-widget -p flower-doc-editor` for the widget alone.
- corgi keeps a test pass until something the test read changes, and the
  environment is not one of those things. A run that sets a `FLOWER_*`
  variable (`FLOWER_THEME`, `FLOWER_TRACE`, `FLOWER_PDF_OUT`) needs
  `corgi test --force`; `scripts/cargo.sh test` adds it itself.
- corgi shows a compile only its crate's `.rs` files. A file a build script,
  an `include_str!` or a test reads is declared under `[extra-inputs]` in
  `corgi.toml`, or an edit to it leaves a stale pass in the store. The
  libraries and the Python there are unit's and diaryx's, to the hash: keep
  them so.
- A binary may not name its checkout: `env!("CARGO_MANIFEST_DIR")` in a test
  is refused. Read the directory at run time (`crate::manifest_dir()` in
  `apps/flower`), or take the test's working directory, which is its
  package's.
- Never delete or reset the store; `corgi clean` trims it.

## The Mac, from Linux

`apps/flower/src/platform/macos` is compiled only for a Mac. A change to it,
or to anything it calls, made on Linux is at least type-checked for
`aarch64-apple-darwin` before it is called done, and said to be unrun.
