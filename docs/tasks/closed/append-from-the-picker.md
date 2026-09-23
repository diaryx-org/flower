---
title: Append a list item from the picker
description: The picker replaces the value of the row it is opened on and nothing else; a list whose items have a vocabulary (a relation with candidates, an enum under `EachItem`) can answer `choices_at` for its append position already, and needs a `begin_choose_append` that commits an `AppendItem` so a reader can add a link by choosing rather than typing
status: done
created: 2026-09-16
updated: 2026-09-23
part_of: '[Closed tasks](/docs/tasks/closed/closed.md)'
---

# Append a list item from the picker

**Status: done** — `Mode::Choosing` carries a `ChoiceTarget` (`Replace` or
`Append`), `Model::begin_choose_append` opens it on a list and falls back to
`begin_append`, a blank editor for a new item (`EditSlot::NewItem`); either
commit lands the cursor on the new item, on its own page if the list was a row
to drill into. `Model::append_target` says which list an add on the selection
means. The widget binds `a` and `A`. The body below said `a` already opened
free text; it did not — the widget had no add at all, so both keys are new.
FFI `page_choose_append(id, value_text)` sits beside `page_choose`. Landed in
the commit that sets this status.

**Where this starts.** `0fd1ce9` gave the model a picker: `begin_choose` on
a scalar with a vocabulary opens a `Mode::Choosing`, and `choose_commit`
emits a `ReplaceValue`. `choices_at` already answers for a sequence's append
position — asked about the list, it asks again at the placeholder
`Index(0)`, which is how an `EachItem` rule and a backend's candidates both
reach it — so the list of things a reader could add is computable today.
What is missing is the gesture: `begin_choose` on a container falls through
to `begin_edit`, and there is no way to open the picker *for a new item*.

**What this adds.** `Model::begin_choose_append(seq_path)` opening the same
`Choosing` mode with an `append` flag, and `choose_commit` emitting
`AppendItem { seq_path, value }` when it is set. The widget binds it beside
the existing append affordance (`a` on a list today opens free text — the
same `e`/`E` split, so the picker where there is one and free text on the
capital). FFI `page_choose_append(id, value_text)` beside `page_choose`.

**Why it waits.** provui's first use of candidates is a *reference* list —
`contents`, `see_also` — where the common act is adding a link, not
replacing one. It should land once provui's `ProvBackend::candidates` exists
and the shape of a reference choice (target as the value, title as the
label) has been exercised in the replace case.

**Done when** a document with `tags: [a]` under a closed enum `[a, b, c]`
can gain `b` through the picker with no typing, and a backend whose
`candidates` answers for `contents` sees the same on a reference list, with
the new item at the end and the cursor on it.
