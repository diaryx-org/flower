---
title: The next generated changelog lists leaf's whole history
part_of: '[Tasks](/docs/tasks/tasks.md)'
status: open
description: ship's changelog counts commits since flower's last tag, v0.6.4, and leaf's 918 commits, merged in after it, are all among them
created: 2026-11-11
---
# The next generated changelog lists leaf's whole history

**What.** leaf's history was merged into flower's as an unrelated history
after flower 0.6.4. None of leaf's commits is an ancestor of `v0.6.4`, so
`ship changelog` — git-cliff over the commits since the last tag — lists
every one of them, with their `Behavioural-change:` trailers, as unreleased.
leaf's own tags (to `v0.5.1`) are not in this repository, and cannot be:
both repositories used `vX.Y.Z`.

**What it should say instead.** Only what has changed since the merge; what
leaf released before it is `docs/CHANGELOG-leaf.md`.

**Ways to get there.** Tell git-cliff where leaf's side stops — the merge's
second parent, `dc2e9b0`, as a skipped ancestor in `cliff.toml` or a tag
there under a name that is not a version (`leaf-0.5.1`) which the config
treats as a boundary — or write the first combined release's section by
hand. Whichever, before the first `ship <version>` here.
