---
title: A spindle's test job cannot finish from an empty cache
part_of: '[Tasks](/docs/tasks/tasks.md)'
status: open
description: Compiling the workspace's tests from nothing takes longer than the hosted spindle's time limit, so the Test job passes only once a timed-out run has saved its cache
---

**What.** `.tangled/workflows/test.yml` runs `ship ci test` on Tangled's
hosted spindle, whose machine for this repository is 2 vCPUs and 4 GiB
(`image: nixos`; `nixos-large` is refused: "workflows in this repository can
use at most 4 GiB"). On 2026-11-11 its first run, with nothing cached, was
stopped as a timeout after 16 minutes, still compiling. The run saved its
`target/` all the same, and the next one compiled the rest and ran the tests in
under 4 minutes. The other five jobs finish cold in 10 minutes or less.

**Why it matters.** The cache is keyed on `Cargo.lock` with a fallback to the
last one saved, so a lockfile change still starts warm. But if the cache is
ever dropped (the spindle's operator may, and a toolchain bump in
`rust-toolchain.toml` rebuilds everything), the Test job times out once, and
perhaps twice, before it can pass again, and nothing in its log says that a
rerun is the cure.

**Done when.** The job passes from an empty cache, or says what to do when it
cannot. Ways there: split the tests over two workflow files (the apps, and the
crates); a spindle of our own with a longer limit; or asking Tangled for a
larger machine for this repository.
