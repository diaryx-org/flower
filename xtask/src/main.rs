//! flower's CI, as one program.
//!
//! Every job the CI workflow runs is one entry in [`JOBS`] and one
//! `cargo xtask <id>` invocation. The workflow itself holds no build knowledge:
//! it asks `cargo xtask ci-matrix` what the jobs are, then runs each one by id.
//! Adding, renaming, reordering, or retiring a job is an edit to this file and
//! nothing else — the YAML does not change.
//!
//! Locally, `cargo xtask ci` runs the same jobs in the same order against the
//! same commands, so a green run here is a green run there.
//!
//! Cutting a release lives here too, in [`release`], for the same reason: the
//! publish workflow asks the program what to publish rather than holding a list
//! of crates that goes stale the moment the workspace gains one.
//!
//! There are no dependencies on purpose. Every CI job builds this crate before
//! it can start, so its build time is paid several times over per push.
//!
//! The Swift half of flower — `packages/flower-swift`, and the AppKit app in
//! `apps/flower-editor` — is mostly not in this table. Compiling and testing it
//! needs macOS and an Xcode toolchain; `scripts/check-swift.sh` and
//! `scripts/test-swift.sh` are run by hand on a Mac until CI grows a macOS
//! runner, at which point they become two more rows below. The one exception is
//! `bindings`: the committed UniFFI Swift binding is generated *from Rust*
//! metadata, so a Linux runner can regenerate and diff it without compiling any
//! Swift.
//!
//! Cutting a release does not live here. It is `release <command>`, from
//! diaryx-org/devtools, configured by `.config/release.toml` — the same tool
//! flower, prov, twig, leaf, and the historica repos all cut releases with,
//! because five copies of one program is five places for it to drift.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Anything that goes wrong here is a message for whoever is reading the log;
/// there is nothing for a CI runner to recover from.
type Result<T> = std::result::Result<T, String>;

/// One CI job: what to call it, what the runner must install for it, and the
/// work itself.
struct Job {
    /// `cargo xtask <id>`, and the key the workflow dispatches on.
    id: &'static str,
    /// The name GitHub shows in the checks list. Renaming it renames the
    /// required status check, so branch protection has to be updated to match.
    name: &'static str,
    /// rustup components the job needs, comma-joined for
    /// `dtolnay/rust-toolchain`. Empty means the default toolchain is enough.
    components: &'static str,
    /// Does this job *compile* the workspace? If so the runner needs the pinned
    /// Zig toolchain — flower's `fig` dependency is Zig-backed and its build.rs
    /// runs `zig build` — and restoring the cargo cache is worth its cost.
    /// `fmt` is the one job that only ever parses.
    builds: bool,
    /// One line of explanation, printed by `cargo xtask` with no arguments.
    about: &'static str,
    run: fn(&Sh) -> Result<()>,
}

/// The whole of CI, in the order `cargo xtask ci` runs it: cheapest and most
/// likely to fail first.
const JOBS: &[Job] = &[
    Job {
        id: "fmt",
        name: "Format",
        components: "rustfmt",
        builds: false,
        about: "rustfmt, in check mode",
        run: fmt,
    },
    Job {
        id: "clippy",
        name: "Clippy",
        components: "clippy",
        builds: true,
        about: "clippy over every target, warnings denied",
        run: clippy,
    },
    Job {
        id: "test",
        name: "Test",
        components: "",
        builds: true,
        about: "the workspace test suite",
        run: test,
    },
    Job {
        id: "package-isolation",
        name: "Package isolation",
        components: "",
        builds: true,
        about: "build each crate alone, without workspace feature unification",
        run: package_isolation,
    },
    Job {
        id: "bindings",
        name: "Swift bindings",
        components: "",
        builds: true,
        about: "the committed UniFFI Swift binding matches crates/flower-ffi",
        run: bindings,
    },
    Job {
        id: "msrv",
        name: "MSRV",
        components: "",
        builds: true,
        about: "build on the minimum supported Rust version",
        run: msrv,
    },
];

// ---------------------------------------------------------------------------
// The jobs
// ---------------------------------------------------------------------------

fn fmt(sh: &Sh) -> Result<()> {
    sh.cargo(&["fmt", "--all", "--check"])
}

/// Warnings are errors in CI, so they are errors here too — a lint that only
/// fires on the runner is a lint found too late.
fn clippy(sh: &Sh) -> Result<()> {
    sh.cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ])
}

fn test(sh: &Sh) -> Result<()> {
    sh.cargo(&["test", "--workspace"])
}

/// Workspace feature unification means `cargo check --workspace` can pass even
/// when a crate cannot compile on its own — some other member's feature
/// selection quietly fills the gap. Each entry below builds one crate in
/// isolation, which is what catches that before it becomes a publish-time
/// surprise.
///
/// A new workspace member belongs in this list; the test at the bottom of this
/// file is what says so when one is forgotten.
const ISOLATED: &[&[&str]] = &[
    &["-p", "flower-core"],
    &["-p", "flower-ratatui"],
    &["-p", "flower-tui"],
    &["-p", "flower-ffi"],
];

fn package_isolation(sh: &Sh) -> Result<()> {
    for spec in ISOLATED {
        let mut args = vec!["check"];
        args.extend_from_slice(spec);
        sh.cargo(&args)?;
    }
    Ok(())
}

/// The Swift package is consumed by version from a bare git checkout, so its
/// UniFFI binding under `packages/flower-swift/uniffi-generated/` is committed —
/// and can therefore go stale against `crates/flower-ffi`. The script
/// regenerates from the crate's embedded metadata and diffs; the output is
/// arch-neutral source, which is what lets this row run on a Linux runner while
/// the rest of the Swift half waits for a macOS one.
fn bindings(sh: &Sh) -> Result<()> {
    sh.run("bash", &["scripts/gen-bindings.sh", "--check"])
}

/// Build on the crate's declared minimum supported Rust version. A build, not a
/// test run: MSRV is a promise about who can *compile* flower, and the
/// dev-dependencies and test tooling need not hold to it.
///
/// The version is read from `workspace.package.rust-version`, so the pin can
/// never drift from the declared floor — bump it in Cargo.toml and this follows.
fn msrv(sh: &Sh) -> Result<()> {
    let version = sh.workspace_rust_version()?;
    println!("MSRV from Cargo.toml: {version}");
    // Idempotent: rustup reports an already-installed toolchain and returns 0.
    sh.run(
        "rustup",
        &[
            "toolchain",
            "install",
            &version,
            "--profile",
            "minimal",
            "--no-self-update",
        ],
    )
    .map_err(|e| format!("{e}\n\nthe MSRV job needs rustup on PATH to pin Rust {version}"))?;
    // `rustup run`, not `cargo +{version}`: the `+toolchain` shorthand is a
    // rustup-proxy feature, and $CARGO may well point past the proxy at a real
    // toolchain binary that does not understand it.
    sh.run(
        "rustup",
        &["run", &version, "cargo", "build", "--workspace"],
    )
}

// ---------------------------------------------------------------------------
// Driving them
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let sh = Sh::new();

    let outcome = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] | ["-h" | "--help" | "help"] => {
            print!("{}", usage());
            return ExitCode::SUCCESS;
        }
        ["ci"] => ci(&sh),
        ["ci-matrix"] => {
            println!("{}", ci_matrix());
            Ok(())
        }
        // These moved to the shared tool rather than being retired, and a
        // muscle-memory `cargo xtask release` should say where they went.
        [
            command @ ("version" | "bump" | "changelog" | "publish" | "release" | "release-notes"),
            ..,
        ] => Err(format!(
            "releasing moved out of xtask: `cargo xtask {command}` is now \
                 `release {command}`,\nthe shared tooling this repo configures in \
                 .config/release.toml.\n\n{}",
            usage()
        )),
        [id] => match JOBS.iter().find(|job| job.id == id) {
            Some(job) => (job.run)(&sh),
            None => Err(format!("unknown job `{id}`\n\n{}", usage())),
        },
        [id, ..] => Err(format!("`{id}` takes no arguments\n\n{}", usage())),
    };

    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("\nxtask: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Every job, in order — what CI does, on one machine. Stops at the first
/// failure, on the theory that a red build is worth reading before the next one
/// buries it.
fn ci(sh: &Sh) -> Result<()> {
    for job in JOBS {
        println!("\n\x1b[1m━━ {} ━━\x1b[0m", job.name);
        (job.run)(sh)?;
    }
    println!("\n\x1b[32mall {} jobs passed\x1b[0m", JOBS.len());
    Ok(())
}

/// The job table as a single line of JSON, for the workflow's `strategy.matrix`.
///
/// Hand-rolled rather than serde-derived: the crate has no dependencies, and
/// every value here is a `&'static str` literal from [`JOBS`] with nothing in it
/// that JSON would need escaped. A job name with a quote or a backslash in it
/// would produce invalid JSON, and `cargo xtask ci-matrix` in the test below is
/// what would notice.
fn ci_matrix() -> String {
    let entries: Vec<String> = JOBS
        .iter()
        .map(|job| {
            format!(
                r#"{{"id":"{}","name":"{}","components":"{}","builds":{}}}"#,
                job.id, job.name, job.components, job.builds
            )
        })
        .collect();
    format!("[{}]", entries.join(","))
}

fn usage() -> String {
    let mut out = String::from(
        "flower's CI, and its releases. Each job below is exactly what the CI \
         workflow runs.\n\n\
         usage: cargo xtask <command>\n\njobs:\n\n",
    );
    for job in JOBS {
        out.push_str(&format!("  {:<18}{}\n", job.id, job.about));
    }
    out.push_str(&format!("  {:<18}{}\n", "ci", "every job above, in order"));
    out.push_str(&format!(
        "  {:<18}{}\n",
        "ci-matrix", "the job table as JSON, for the workflow matrix"
    ));
    // Releasing is not CI and is not here: it is one shared tool across the
    // org, so that the changelog contract has one implementation rather than
    // five that agree until they don't.
    out.push_str(
        "\nreleasing:  release <command>   (diaryx-org/devtools; see .config/release.toml)\n",
    );
    out
}

// ---------------------------------------------------------------------------
// Running things
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// The workspace
// ---------------------------------------------------------------------------
//
// This came across from `release.rs` when releasing moved to the shared tooling.
// Only the isolation test still asks the question, and it is a CI question: a
// crate added to the workspace and left out of `ISOLATED` is built by
// `--workspace` and by nothing alone.

#[cfg(test)]
/// A workspace member: where its manifest lives, and what the registry calls it.
///
/// flower keeps its crates under `crates/`, so a member's path and its package
/// name are two different strings — `crates/flower-core` is read, `flower-core`
/// is what `cargo publish -p` and crates.io are asked about. Conflating them is
/// how a publish ends up looking for a crate named after a directory.
struct Member {
    path: String,
    name: String,
    /// `publish = false` in the member's `[package]` table.
    publishable: bool,
}

#[cfg(test)]
/// The workspace members, in manifest order.
///
/// The `members` array is read across however many lines it spans, from the key
/// to the closing bracket, so the list can stay one-per-line and commented.
fn members(sh: &Sh) -> Result<Vec<Member>> {
    let manifest = sh.read("Cargo.toml")?;
    let start = manifest
        .find("members")
        .ok_or_else(|| "no `members` in [workspace]".to_string())?;
    let rest = &manifest[start..];
    let end = rest
        .find(']')
        .ok_or_else(|| "unterminated `members` array in [workspace]".to_string())?;

    let mut out = Vec::new();
    for path in rest[..end].split('"').skip(1).step_by(2) {
        let text = sh.read(&format!("{path}/Cargo.toml"))?;
        let name = package_name(&text)
            .ok_or_else(|| format!("no `name` in {path}/Cargo.toml's [package] table"))?;
        let publishable = !text.lines().any(|line| {
            let line = line.trim();
            line.starts_with("publish") && line.contains("false")
        });
        out.push(Member {
            path: path.to_string(),
            name,
            publishable,
        });
    }
    Ok(out)
}

#[cfg(test)]
/// `name = "flower-core"` from a member manifest. The first such line: a
/// `[dependencies]` entry is `flower-core = …`, never `name = …`, so nothing
/// below `[package]` can be mistaken for it.
fn package_name(manifest: &str) -> Option<String> {
    manifest
        .lines()
        .find(|line| line.trim_start().starts_with("name = \""))
        .and_then(|line| line.split('"').nth(1))
        .map(str::to_owned)
}

/// A shell rooted at the workspace, so a job never has to think about where it
/// was invoked from.
struct Sh {
    root: PathBuf,
    /// Cargo tells its subprocesses which cargo it is; prefer that over
    /// whichever one happens to be first on PATH.
    cargo: String,
}

impl Sh {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask/ always has a parent")
            .to_path_buf();
        let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        Sh { root, cargo }
    }

    fn cargo(&self, args: &[&str]) -> Result<()> {
        let cargo = self.cargo.clone();
        self.run(&cargo, args)
    }

    /// Run a command at the workspace root, echoing it first so a CI log reads
    /// as a transcript of commands anyone can paste back.
    fn run(&self, program: &str, args: &[&str]) -> Result<()> {
        let shown = if program == self.cargo {
            "cargo"
        } else {
            program
        };
        println!("\x1b[2m$ {} {}\x1b[0m", shown, args.join(" "));

        let status = Command::new(program)
            .args(args)
            .current_dir(&self.root)
            .status()
            .map_err(|e| format!("could not run `{shown}`: {e}"))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("`{shown} {}` failed ({status})", args.join(" ")))
        }
    }

    /// Read a workspace file, by its path from the root. Test-only since
    /// releasing moved out: the isolation test reads the manifests, and no job
    /// touches a file directly.
    #[cfg(test)]
    fn read(&self, path: &str) -> Result<String> {
        let path = self.root.join(path);
        std::fs::read_to_string(&path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))
    }

    /// `workspace.package.rust-version`, the single source of truth for the MSRV.
    fn workspace_rust_version(&self) -> Result<String> {
        let manifest = self.root.join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest)
            .map_err(|e| format!("could not read {}: {e}", manifest.display()))?;
        text.lines()
            .find_map(|line| line.trim().strip_prefix("rust-version"))
            .and_then(|rest| rest.split('"').nth(1))
            .map(str::to_owned)
            .ok_or_else(|| format!("no `rust-version` in {}", manifest.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The workflow's `fromJSON` is the only thing that parses `ci-matrix`, and
    /// it fails at a point where the fix costs a push. Check the shape here
    /// instead: one object per job, every field present, nothing needing an
    /// escape.
    #[test]
    fn ci_matrix_is_well_formed_json() {
        let json = ci_matrix();
        assert!(json.starts_with('[') && json.ends_with(']'));
        assert_eq!(json.matches("\"id\":").count(), JOBS.len());
        assert_eq!(json.lines().count(), 1, "the workflow reads it as one line");

        for job in JOBS {
            for field in [job.id, job.name, job.components] {
                assert!(
                    !field.contains(['"', '\\']),
                    "`{field}` would need JSON escaping, which ci_matrix does not do",
                );
            }
            assert!(json.contains(&format!("\"id\":\"{}\"", job.id)));
        }
    }

    /// `ci` and `ci-matrix` are handled before the table is consulted, so a job
    /// by either name would be unreachable.
    #[test]
    fn job_ids_are_distinct_and_dispatchable() {
        let mut ids: Vec<&str> = JOBS.iter().map(|job| job.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate job id");
        assert!(!ids.contains(&"ci") && !ids.contains(&"ci-matrix"));
    }

    /// Every member of the workspace should be built alone by the isolation
    /// job; that is the whole point of it. `xtask` is exempt — it is this
    /// program, and every job has already built it by the time one runs.
    #[test]
    fn package_isolation_covers_every_member() {
        let sh = Sh::new();
        for member in members(&sh).unwrap() {
            if member.name == "xtask" {
                continue;
            }
            assert!(
                ISOLATED
                    .iter()
                    .any(|spec| spec == &["-p", member.name.as_str()]),
                "workspace member `{}` is not built in isolation by `cargo xtask package-isolation`",
                member.name,
            );
        }
    }

    /// A member's path and its package name are different strings here, so the
    /// name has to come from the manifest rather than from the directory.
    #[test]
    fn package_names_come_from_the_manifest() {
        assert_eq!(
            package_name("[package]\nname = \"flower-core\"\nedition.workspace = true\n")
                .as_deref(),
            Some("flower-core")
        );
        assert_eq!(package_name("[workspace]\nresolver = \"3\"\n"), None);
    }

    /// The real workspace: every member found, named, and read. The `members`
    /// array spans several lines here, which is the case a `find`-to-newline
    /// reader gets wrong and this one does not.
    #[test]
    fn members_are_read_across_the_whole_array() {
        let found = members(&Sh::new()).unwrap();
        assert!(
            found.iter().any(|m| m.path == "crates/flower-core"
                && m.name == "flower-core"
                && m.publishable)
        );
        // The binding crate publishes too: its view projection is generic over
        // the backend, so an embedder with its own needs to depend on it.
        assert!(
            found
                .iter()
                .any(|m| m.name == "flower-ffi" && m.publishable),
        );
        assert!(
            found
                .iter()
                .any(|m| m.name == "flower-ratatui" && !m.publishable),
            "flower-ratatui is publish = false",
        );
        assert!(found.iter().any(|m| m.name == "xtask" && !m.publishable));
        assert!(found.len() >= 5, "the array spans several lines");
    }

    /// The MSRV job reads this; if the parse breaks, the job silently pins the
    /// wrong compiler or fails far from the cause.
    #[test]
    fn msrv_is_readable_from_the_manifest() {
        let version = Sh::new().workspace_rust_version().unwrap();
        assert!(
            version.split('.').all(|part| part.parse::<u32>().is_ok()),
            "`{version}` does not look like a Rust version",
        );
    }
}
