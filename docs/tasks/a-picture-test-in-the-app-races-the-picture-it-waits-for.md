---
title: A picture test in the app races the picture it waits for
part_of: '[Tasks](/docs/tasks/tasks.md)'
status: open
description: headless::blitz::a_piece_keeps_its_pictures_shape_until_the_next_is_loaded failed once on a spindle because the new picture had already loaded when the test looked for the old height
---

**What.** `headless::blitz::a_piece_keeps_its_pictures_shape_until_the_next_is_loaded`
(`apps/flower/src/headless.rs`) gives a node a taller picture, renders once,
and asserts the node still has the old picture's height, 40, before waiting for
80. On Tangled's spindle (2 vCPUs) it failed once on 2026-11-11 with 80: the
new picture had loaded before the assertion ran. It passed in the runs before
and after, and has not failed on a workstation.

**Why it matters.** The gate runs on that spindle for every push to `main`
and every pull request, so a race there is a red run that says nothing about
the change.

**Done when.** The test decides when the picture loads, instead of hoping to
look before it does: a net provider the test releases by hand would do it.
