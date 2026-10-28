---
title: The Dioxus CI job has no fontconfig
part_of: '[Closed tasks](/docs/tasks/closed/closed.md)'
status: done
created: 2026-10-21
updated: 2026-10-28
description: 'The `dioxus` job added to CI on 2026-10-19 has failed on every push to main since: fontique, reached through dioxus-native, builds yeslogic-fontconfig-sys, which needs `fontconfig.pc`, and the runner installs nothing but Rust'
---

# The Dioxus CI job has no fontconfig

**Status: done**: the first choice. A job's `runner` table takes
`apt`, the Ubuntu packages to install, and ship already hands any key of
that table through `ship ci --matrix`. `ci.yml` installs them before the
cache step, and the `dioxus` job asks for `libfontconfig-dev`. Its `run` is
unchanged. fontconfig was the only one: wayland-sys is built with `dlopen`,
and the image has OpenSSL. The same change took dependencies' debug info
out of both Dioxus workspaces' dev profile, which halves the job's ~14 GB of
`target/`.

**Repro.** `CI / Dioxus widget and app` is red on main from `6c87174`
(2026-10-20) on, and every other job is green. The job stops in the first
`cargo clippy`, in a build script:

```text
yeslogic-fontconfig-sys-6.0.1/build.rs:8:48:
  Package fontconfig was not found in the pkg-config search path.
  The system library `fontconfig` required by crate `yeslogic-fontconfig-sys` was not found.
```

The same happens on any Linux machine where pkg-config cannot find
fontconfig:

```sh
cd crates/flower-dioxus
PKG_CONFIG_LIBDIR=$(mktemp -d) PKG_CONFIG_PATH= cargo build -p yeslogic-fontconfig-sys
```

**Why.** `cead79e` added the `dioxus` job to `.config/ship.figl`. Both
`crates/flower-dioxus` and `apps/flower-dioxus` take `dioxus-native`, whose
`system-fonts` feature reaches `fontique` through blitz and parley, and on
Linux fontique links fontconfig through `yeslogic-fontconfig-sys`. The
workflow installs a Rust toolchain and the job's rustup components, and a
job's `runner` table has nothing to ask for a system package with, so
`ubuntu-latest` has to have `libfontconfig-dev` already, and it does not.

**The choice.** Either:

- ship's `runner` table gains a key for system packages (say
  `apt = libfontconfig-dev`), `ship ci --matrix` hands it through, and
  `.github/workflows/ci.yml` installs it before the cache step. The job's
  `run` stays what `ship ci dioxus` runs here, with no `sudo` in it. Or
- the `dioxus` job's `run` begins with an install, which works on the
  runner but makes `ship ci` ask for `sudo` on a developer's machine.

fontconfig is the first library the build stops at, and may not be the
last: the app also builds `openssl-sys`, which the runner image has had so
far.

## Done when

`CI / Dioxus widget and app` passes on main, and `ship ci dioxus` still
runs unchanged on a machine that has fontconfig.
