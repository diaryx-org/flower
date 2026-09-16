---
title: flower
nav_title: flower
nav_order: 50
description: flower — a structural editor for config files. Navigate the parsed tree, edit one value at a time, never hold an invalid document.
audience: public
part_of: '[flower](/README.md)'
id: v817zvg
---
<section class="pj-head">
  <div class="wrap">
    <p><a class="crumb" href="../about/#projects">diaryx.org / projects /</a></p>
    <div class="pj-title" style="margin-top: 1rem">
      <h1>flower</h1>
      <span class="pj-tags">
        <span class="tag-chip">Rust</span>
        <span class="tag-chip">MIT / Apache-2.0</span>
      </span>
    </div>
    <p class="pj-tagline">
      A structural editor for config files — it edits the tree, not the
      characters.
    </p>
  </div>
</section>

<section class="pj-main">
<div class="wrap pj-layout reveal">
<div class="pj-body">

Where a text editor edits characters, flower edits the tree. You
navigate the parsed config structurally — into mappings, along
sequences, down to scalars — and edit one value at a time. Every
change is a path-addressed, lossless splice through fig's editor:
the bytes you didn't touch stay byte-for-byte identical, and the
document is only ever committed in a valid state.

- **Two views of the same tree.** An indented, type-colored tree; or a settings-menu page view that sinks the fields nobody types below the ones they do.
- **Typed scalar edits.** Booleans, numbers, nulls, and text are committed through fig's validated replace — not typed freehand.
- **Everything fig reads.** JSON/JSONC/JSON5, YAML, TOML, ZON, and the fig dialect.

## Where it fits

flower is the config sibling of [leaf](id:leaf/z4z2w13):
leaf's caret model is right for permissive document formats; a
strict config grammar wants a structural one. It edits through
[fig](id:fig/gns15jg)'s tree, and in
[Diaryx](id:org/80k72t9) it's a natural front end for vault
configuration and frontmatter.

## Status

Early prototype. Navigation, both views, typed edits, deletes, and
save all work today; the roadmap in the repo says what isn't here
yet.

</div>
<aside class="pj-aside">
<div class="install">
<span class="install-head">Install</span>
<div class="cmd">cargo add flower-core <small>Rust</small></div>
</div>
<div class="facts">
<div class="row"><span class="k">Language</span><span class="v">Rust (on fig)</span></div>
<div class="row"><span class="k">Built on</span><span class="v"><a href="../fig/index.md">fig</a></span></div>
<div class="row"><span class="k">Used by</span><span class="v"><a href="../index.html">Diaryx</a> — config &amp; metadata editing</span></div>
<div class="row"><span class="k">Source</span><span class="v"><a href="https://github.com/diaryx-org/flower">github.com/diaryx-org/flower</a></span></div>
<div class="row"><span class="k">Packages</span><span class="v"><a href="https://crates.io/crates/flower-core">crates.io/crates/flower-core</a></span></div>
<div class="row"><span class="k">License</span><span class="v">MIT or Apache-2.0</span></div>
</div>
</aside>
</div>
</section>
