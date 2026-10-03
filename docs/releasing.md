# Releasing flower

The whole workspace shares one version number, one tag, and one changelog. A
release is therefore one command:

```console
$ release release minor          # bump, changelog, commit, tag
$ release release minor --push   # …and push, which ships the binaries
```

`release` is the shared tooling in [diaryx-org/devtools][devtools], which
flower, prov, twig, leaf, and the historica repos all cut releases with. What
makes flower flower is `.config/release.toml` and nothing else; the behaviour
lives there. It used to be `cargo xtask release`, one of five copies of the same
program.

[devtools]: https://github.com/diaryx-org/devtools

Everything below is what that command does, and what it deliberately refuses to
do on its own.

## How flower reaches its consumers

From git, not crates.io; 0.6.4 was the last version uploaded there. provui and
the Diaryx app name the crates they use the same way:

```toml
flower-core = { git = "https://github.com/diaryx-org/flower", branch = "main" }
```

and each one's `Cargo.lock` records the exact commit it builds, which `cargo
update -p flower-core` moves. So a change reaches them once it is pushed to
`main`, not once it is released. The spelling has to match everywhere: cargo
tells sources apart by it, and one consumer on a `rev` while another is on
`branch = "main"` is two flower-cores in one graph.

The crates other repositories use are three:

- **`flower-core`** — the frontend-neutral structural editing model.
- **`flower-ratatui`** — the ratatui widget. `draw` and `handle_key` are the
  whole of it, and the `Outcome` `handle_key` returns is what lets a host own
  the event loop rather than surrender it. It draws into a `Rect`, not only
  into the whole frame, so a terminal app with a config pane can put the editor
  in that pane. Pinned to one ratatui minor, which is the cost of the crate
  being useful at all.
- **`flower-ffi`** — the UniFFI binding. It is a staticlib for the Swift app,
  but it is also a Rust API: [`view_of`]/[`pages_of`]/[`path_for_id`] and the
  flat view records they build are generic over the `Backend`, and an embedder
  with a backend of its own (a prov document's embedded metadata, say) needs
  them. A UniFFI *object* cannot be generic, so the `FlowerDoc` handle stays
  nailed to `FigBackend` — the projection is the reusable half. `leaf-ffi` is a
  library for the same reason.

**`flower-tui`** is the prototype binary, run from a checkout (`cargo run -p
flower-tui -- path/to/config.toml`). It moves with the workspace version, and
appears in the changelog.

## What a tag starts

Pushing `vX.Y.Z` starts **`homebrew.yml`**, which builds the TUI and points the
tap's `flower` formula at it, and **`mac-app.yml`**, which runs `cargo xtask
package` on a macOS runner — Flower.app signed for Developer ID, notarised and
stapled, in a `.dmg` — attaches the image to the release, and points the tap's
`flower-editor` cask at it. Dispatched by hand, `mac-app.yml` builds whatever
ref it is run on and keeps the image as a workflow artifact, touching no
release: the rehearsal. Dispatched with `cask-tag`, it only rewrites the cask
for a release that already has its image.

That is why `release` stops at the local tag unless it is given `--push`: every
step before the push is a commit you can amend or throw away, and the push is the
step that ships. Without `--push` the command prints the two
`git push` lines it did not run, and the two-line undo.

## What `release` checks first

`release release` refuses before it writes anything if the working tree is
dirty, the branch is not `main`, `main` is behind `origin/main`, the tag
already exists locally or on origin, or git-cliff is not installed.

Then it runs the whole of CI (`cargo xtask ci`), the same jobs the workflow runs.
`--no-verify` skips that, and is for a release you have just watched go green.

## The pieces, on their own

| Command | What it does |
|---|---|
| `release version` | print the workspace version |
| `release bump <patch\|minor\|major\|x.y.z\|as-is>` | move `[workspace.package]`, every internal `path`+`version` dependency, and the lockfile |
| `release changelog` | print the generated region |
| `release changelog --write` | splice it into `docs/CHANGELOG.md` |
| `release changelog --check` | fail if that region is stale |
| `release release-notes [tag]` | that release's changelog section, as a GitHub release body |

## The changelog

`docs/CHANGELOG.md` is handwritten except for one region, between

```
<!-- git-cliff:begin — generated; edits here are overwritten -->
<!-- git-cliff:end -->
```

inside `## Unreleased`. git-cliff fills it from the commits since the last tag
through the shared `cliff.toml` in diaryx-org/devtools; `release` renders it one
last time, moves it into a `## vX.Y.Z — date` section, and empties the region.
Edits inside the markers are lost on the next write. A release **intro** — for a
release that wants a narrative rather than a list — goes in the released section
below the end marker, where regeneration cannot reach it.

`--write` also answers to the tag list, not just to the region: any `v*` tag with
no `## <tag> —` section of its own gets one, generated from its commit range and
folded in at its place in the order. That is what a tag cut *after* the fact
needs — without it, tagging those commits would make them vanish from the file
entirely: no longer unreleased, and in no section either. `--check` reports a
missing section the same way it reports a stale region. Existing sections are
never rewritten, so a handwritten intro survives every write.

There is no CI job checking the region for staleness: it is regenerated as part
of every release, and git-cliff is not on the runners.

## What the commits have to say

Two conventions carry straight into the changelog.

**Spell the colon.** `add(model): offer the schema's declared fields the document
lacks`, not `add(model) offer …`. Much of flower's log drops it, and
git-conventional then cannot tell where the subject ends — the whole commit body
lands in the bullet and the trailers below it are never parsed as trailers. A
preprocessor in the shared `cliff.toml` in diaryx-org/devtools puts the colon
back for the known types so the existing history still reads, but it is a rescue,
not a licence.

`add` is the house spelling of `feat`; `polish` and `move` ride with `refactor`.
`docs`, `chore`, `test`, `ci`, `build`, and `style` are skipped entirely, and
anything the parsers do not recognise lands in an **Uncategorised — triage before
release** bucket rather than being dropped.

**Write a `Behavioural-change:` trailer** on any commit where a caller who
upgrades without editing a line of their own code would observe a difference — a
field that appears, an error that stops being returned, a row that renders
differently. It is true of a bug fix as often as of a feature. The trailers are
collected, in commit order, into a **Behavioural changes** section at the end of
the release, which is the part a consumer reads first and often only.

```
add(model): a value the schema declares read-only can no longer be staged

Behavioural-change: `Model::set_value_at` returns `Err` on a path the schema
  declares read-only. It used to accept the edit and drop it silently at
  commit time.
```

One trailer per observable difference; a commit may carry several. Continuation
lines are indented two spaces.

## CI, for the same reason

`xtask` holds CI too, and for the same reason it holds releases: the workflow
should not know things the manifests already say. `cargo xtask ci` runs every job
locally, in the workflow's order; `cargo xtask <id>` runs one.

| Job | What it runs |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `clippy` | `cargo clippy --workspace --all-targets -- -D warnings` |
| `test` | `cargo test --workspace` |
| `package-isolation` | `cargo check -p <crate>` for each member, so workspace feature unification cannot hide a crate that fails to build alone |
| `app-version` | reads `apps/flower-editor/project.yml` and checks its `MARKETING_VERSION` is the workspace version (`cargo xtask sync-versions` writes it, as the bump's `post_bump`) |
| `msrv` | a `--workspace` build on `workspace.package.rust-version` |

Adding or renaming a job is an edit to `xtask/src/main.rs` and nothing else — the
workflow reads the table from `cargo xtask ci-matrix`. Renaming one renames the
required status check, so branch protection has to follow.

The Swift half (`packages/flower-swift`, `apps/flower-editor`) is mostly not in
that table: compiling it needs macOS and Xcode. `scripts/check-swift.sh`
type-checks FlowerUI against the generated binding, and `scripts/test-swift.sh`
runs its XCTest bundle; run them on a Mac. They become two more rows in `JOBS`
the day CI grows a macOS runner. The exception is the `bindings` row, which
regenerates the *committed* UniFFI binding under
`packages/flower-swift/uniffi-generated/` from `crates/flower-ffi` and diffs —
Rust-only work that fits the Linux runner.
