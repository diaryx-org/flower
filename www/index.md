---
title: flower
nav_title: flower
nav_order: 40
description: flower — an editor for documents and data files that edits the syntax tree. A caret for Markdown, Djot and HTML; a tree for JSON, YAML and TOML.
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
        <span class="tag-chip">Rust · Swift</span>
        <span class="tag-chip">MIT / Apache-2.0</span>
      </span>
    </div>
    <p class="pj-tagline">
      An editor for documents and data files that edits the syntax tree:
      a caret for Markdown, Djot and HTML, a tree for JSON, YAML and TOML.
    </p>
  </div>
</section>

<section class="pj-main">
<div class="wrap pj-layout reveal">
<div class="pj-body">

flower is one editor for two kinds of file. A data file — JSON, YAML,
TOML, ZON, figl — opens as its tree: you move into mappings and along
lists and edit one value at a time, each change a lossless splice
through fig, so comments, key order and quoting stay as they were. A
document's frontmatter opens the same way, from the document's own
window.

For a document, flower gives you an ordinary text caret — mouse, selection, a
formatting toolbar — and turns every keystroke into an
offset-addressed edit on twig's tree. The document stays a live,
round-trippable AST the whole time you type into it, so a Markdown
file and a Djot file are edited through exactly the same
operations.

- **Two views, one rendering path.** Source (caret in raw bytes) and WYSIWYG (`**bold**` painted bold, delimiters hidden) — every rendered glyph maps back to its source byte, so carets and clicks ride the visible text in either.
- **One core, many frontends.** A frontend-neutral core; embeddable widgets for ratatui, gpui, AppKit/UIKit (via UniFFI), and the browser (via wasm).
- **Thin hosts everywhere.** A terminal editor, a GUI app, an iOS host, and a web demo are all thin wrappers around the same widget.

## Where it fits

flower is the editor underneath [Diaryx](id:dx4hxn744/80k72t9)'s writing
surface — the same core that renders an entry on Mac renders it on
iPhone. It edits documents through [twig](id:dx94n1gx2/wxmq0ww)'s
tree. Standalone, it's embeddable anywhere a rich-text view over
plain files belongs.

## Status

Work in progress, and open about it: caret editing, mouse,
selection, and both views work today; soft-wrap edge cases, inline
images, and undo/redo are the known next steps.

</div>
<aside class="pj-aside">
<div class="install">
<span class="install-head">Install</span>
<div class="cmd">cargo add flower-doc <small>Rust</small></div>
</div>
<div class="facts">
<div class="row"><span class="k">Language</span><span class="v">Rust · Swift · TypeScript (web package)</span></div>
<div class="row"><span class="k">Built on</span><span class="v"><a href="../twig/index.md">twig</a></span></div>
<div class="row"><span class="k">Used by</span><span class="v"><a href="../index.html">Diaryx</a> — the writing surface</span></div>
<div class="row"><span class="k">Source</span><span class="v"><a href="https://tangled.org/adammharris.com/flower">tangled.org/adammharris.com/flower</a></span></div>
<div class="row"><span class="k">Packages</span><span class="v"><a href="https://crates.io/crates/flower-doc">crates.io/crates/flower-doc</a></span></div>
<div class="row"><span class="k">License</span><span class="v">MIT or Apache-2.0</span></div>
</div>
</aside>
</div>
</section>
