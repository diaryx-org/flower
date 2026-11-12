---
title: The gate's first run from an empty cache may not fit the spindle's limit
part_of: '[Tasks](/docs/tasks/tasks.md)'
status: open
description: 'On the hosted spindle''s container engine Test and Package isolation take about 15 minutes from an empty cache, which is where a workflow is stopped: they have passed once and been stopped once'
---
# The gate's first run from an empty cache may not fit the spindle's limit

**What.** Tangled's hosted spindle stops a workflow 15 minutes after it
starts, on either engine (measured 2026-11-11 with a workflow that only
sleeps), and nothing in a workflow file can raise that.

On the microVM engine the machine for this repository is 2 vCPUs and 4 GiB
(`nixos-large`, 6 and 8, is refused: "workflows in this repository can use at
most 4 GiB"), and from an empty cache Test and Package isolation were stopped
still compiling. So the six jobs in `.tangled/workflows/` run on the container
engine (`engine: nixery`), where a step saw 20 cores and 64 GB.

There, on 2026-11-11, all six passed from an empty cache, started together:
Format 1 minute, Swift bindings 5, Web host 8, Clippy 10, Test 14 and Package
isolation 14 and a half. Test alone on the host built its binaries in 5
minutes; with the other jobs beside it, in 12. The cores are shared with the
other jobs and with other people's.

The same day, the first push of those workflows to `main` (its caches empty
again, being the microVM's) was a busier hour: Test and Package isolation were
both stopped at 15 minutes. Retried from the caches they had saved, they
passed in 9.

**Why it matters.** A cache is kept per workflow file and branch, and a branch
with none takes `main`'s, so a pull request starts warm and so does a change
to `Cargo.lock` (`fallback: true`). Everything is rebuilt when `main`'s cache
is gone (unused for 30 days, or evicted for room) or when
`rust-toolchain.toml` moves. On that day Test and Package isolation pass or are stopped by how busy the
host is: one of each has been seen. A stopped job still saves its
cache (`when: always`), so `tg pipeline retry` passes; nothing in the log says
so.

**Done when.** The slowest job from an empty cache has minutes to spare, not
one. Ways there: Package isolation is twenty `cargo check`s one after another
and would halve if `cargo xtask package-isolation` took a part of its list and
two workflow files each ran one; Test splits less well, since most of its time
is the dependencies every part would build; or Tangled's operators raise the
account's quota, which is the only way to a longer limit or a larger VM.
