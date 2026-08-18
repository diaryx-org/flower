# Releasing flower

The whole workspace shares one version number, one tag, and one changelog. A
release is therefore one command:

```console
$ cargo xtask release minor          # bump, changelog, commit, tag
$ cargo xtask release minor --push   # …and push, which publishes
```

Everything below is what that command does, and what it deliberately refuses to
do on its own.

## What goes to crates.io

`flower-core` — the frontend-neutral structural editing model — and nothing
else. `flower-ratatui`, `flower-tui`, and `flower-ffi` are `publish = false`:

- **`flower-ratatui`** is a widget pinned to one ratatui minor whose only
  consumer is `flower-tui` in this repo.
- **`flower-tui`** is the prototype binary; run it from a checkout
  (`cargo run -p flower-tui -- path/to/config.toml`).
- **`flower-ffi`** is a UniFFI staticlib for the Swift app, not a Rust API.

They still move with the workspace version, and still appear in the changelog —
publishing is the only thing they are out of. To publish one, delete its
`publish = false`: `cargo xtask publish` derives the list and the order from the
manifests, so nothing else needs editing.

## What a tag starts

Pushing `vX.Y.Z` starts **`publish.yml`**, which runs `cargo xtask publish` and
uploads every publishable crate the registry is missing, in dependency order. A
crates.io version number can be yanked but never reused.

That is why `release` stops at the local tag unless it is given `--push`: every
step before the push is a commit you can amend or throw away, and the push is the
step that spends a version number. Without `--push` the command prints the two
`git push` lines it did not run, and the two-line undo.

## What `release` checks first

`cargo xtask release` refuses before it writes anything if the working tree is
dirty, the branch is not `master`, `master` is behind `origin/master`, the tag
already exists locally or on origin, git-cliff is not installed, or **any crate
is already on crates.io at the target version**. That last one asks the registry
rather than the tag list, because a crate can go up from a laptop without ever
being tagged — the registry is the record of what has been spent.

Then it runs the whole of CI (`cargo xtask ci`), the same jobs the workflow runs.
`--no-verify` skips that, and is for a release you have just watched go green.

## The pieces, on their own

| Command | What it does |
|---|---|
| `cargo xtask version` | print the workspace version |
| `cargo xtask bump <patch\|minor\|major\|x.y.z>` | move `[workspace.package]`, every internal `path`+`version` dependency, and the lockfile |
| `cargo xtask changelog` | print the generated region |
| `cargo xtask changelog --write` | splice it into `docs/CHANGELOG.md` |
| `cargo xtask changelog --check` | fail if that region is stale |
| `cargo xtask publish --list` | the publish order, derived from the manifests |
| `cargo xtask publish` | publish every crate crates.io is missing |
| `cargo xtask release-notes [tag]` | that release's changelog section, as a GitHub release body |

`publish` is idempotent per crate — it asks the registry before each upload — so
a release that died halfway is finished by running it again, locally or by
re-running the workflow.

Auth is the `CARGO_REGISTRY_TOKEN` secret on the repo, as in fig, twig, moid, and
prov. **flower has never published, so its first release needs a token carrying
`publish-new`** as well as `publish-update`, with no crate restriction — a token
missing either fails at the first upload with `403 … token is not valid for crate
flower-core`. `cargo xtask publish` says so when it hits that 403. The secret is
not set on this repo yet; setting it is a prerequisite for the first tag.

## The changelog

`docs/CHANGELOG.md` is handwritten except for one region, between

```
<!-- git-cliff:begin — generated; edits here are overwritten -->
<!-- git-cliff:end -->
```

inside `## Unreleased`. git-cliff fills it from the commits since the last tag
through `.config/cliff.toml`; `release` renders it one last time, moves it into a
`## vX.Y.Z — date` section, and empties the region. Edits inside the markers are
lost on the next write. A release **intro** — for a release that wants a
narrative rather than a list — goes in the released section below the end marker,
where regeneration cannot reach it.

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
preprocessor in `.config/cliff.toml` puts the colon back for the known types so
the existing history still reads, but it is a rescue, not a licence.

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
| `msrv` | a `--workspace` build on `workspace.package.rust-version` |

Adding or renaming a job is an edit to `xtask/src/main.rs` and nothing else — the
workflow reads the table from `cargo xtask ci-matrix`. Renaming one renames the
required status check, so branch protection has to follow.

The Swift half (`packages/flower-swift`, `apps/flower-editor`) is not in that
table: both need macOS and Xcode. `scripts/check-swift.sh` type-checks FlowerUI
against the generated binding, and `scripts/test-swift.sh` runs its XCTest
bundle; run them on a Mac. They become two more rows in `JOBS` the day CI grows a
macOS runner.
