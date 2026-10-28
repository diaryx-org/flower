//! flower's tasks: the ones too big to be a command.
//!
//! CI itself is the `job` list in `.config/ship.figl`, run by `ship ci` here
//! and on the runners. Most of its jobs are plain commands; the one that is
//! real logic runs a task from here — `package-isolation`, which builds each
//! workspace member alone from a list a test keeps whole.
//!
//! The rest are the Mac app's. `cargo xtask mac` puts `Flower.app` together
//! from `apps/flower-dioxus` and launches it ([`app`]); `cargo xtask package`
//! — the signed, notarised `.dmg` that `mac-app.yml` attaches to a release —
//! lives in [`package`].
//!
//! Cutting a release does not live here. It is `ship`, configured by the same
//! `.config/ship.figl`.
//!
//! There are no dependencies on purpose. CI jobs build this crate before they
//! can start, so its build time is paid on every push.

mod app;
mod package;

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Anything that goes wrong here is a message for whoever is reading the log;
/// there is nothing for a CI runner to recover from.
type Result<T> = std::result::Result<T, String>;

/// Workspace feature unification means `cargo check --workspace` can pass even
/// when a crate cannot compile on its own — some other member's feature
/// selection quietly fills the gap. Each entry below builds one crate in
/// isolation, which is what catches that before it becomes a publish-time
/// surprise.
///
/// A new workspace member belongs in this list; the test at the bottom of this
/// file is what says so when one is forgotten. `ship ci package-isolation` runs
/// it.
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

// ---------------------------------------------------------------------------
// Driving the tasks
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let sh = Sh::new();

    let outcome = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] | ["-h" | "--help" | "help"] => {
            print!("{}", usage());
            return ExitCode::SUCCESS;
        }
        ["package-isolation"] => package_isolation(&sh),
        ["mac", ref rest @ ..] => app::mac(&sh, rest),
        ["package", ref rest @ ..] => package::package(&sh, rest),
        // The Swift app these served is gone; the bundle takes its version
        // from the workspace when it is put together.
        [command @ ("swift" | "app-version" | "sync-versions"), ..] => Err(format!(
            "`cargo xtask {command}` went with the Swift app: the Mac app is \
             apps/flower-dioxus, built by `cargo xtask mac`, and its bundle takes \
             the workspace's version when it is put together.\n\n{}",
            usage()
        )),
        // These moved to ship rather than being retired, and a muscle-memory
        // `cargo xtask release` should say where they went.
        [
            command @ ("ci" | "ci-matrix" | "fmt" | "clippy" | "test" | "bindings" | "msrv"),
            ..,
        ] => Err(format!(
            "CI moved out of xtask: `cargo xtask {command}` is now a job in \
                 .config/ship.figl.\n`ship ci` runs every job, `ship ci <id>` one, and \
                 `ship ci --list` names them.\n\n{}",
            usage()
        )),
        [
            command @ ("version" | "bump" | "changelog" | "publish" | "release" | "release-notes"
            | "github-release"),
            ..,
        ] => Err(format!(
            "releasing moved out of xtask: `cargo xtask {command}` is now `{}`,\n\
             configured in .config/ship.figl.\n\n{}",
            ship_spelling(command),
            usage()
        )),
        [id] => Err(format!("unknown task `{id}`\n\n{}", usage())),
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

/// Where a release command that left xtask lives now.
fn ship_spelling(command: &str) -> &'static str {
    match command {
        "version" => "ship version",
        "bump" => "ship bump",
        "changelog" => "ship changelog",
        "release-notes" => "ship notes",
        "github-release" => "ship github",
        // `release` and `publish` are both the cut: `ship <spec>`, which
        // pushes and writes the GitHub release only with --push.
        _ => "ship <patch|minor|major|x.y.z>",
    }
}

fn usage() -> String {
    let mut out = String::from(
        "flower's tasks.\n\n\
         usage: cargo xtask <task>\n\nCI jobs that are programs:\n\n",
    );
    out.push_str(&format!(
        "  {:<18}{}\n",
        "package-isolation", "build each crate alone, without workspace feature unification"
    ));
    out.push_str("\nthe app:\n\n");
    out.push_str(&format!(
        "  {:<18}{}\n",
        "mac [FILE]", "build Flower.app from apps/flower-dioxus and launch it (--help)"
    ));
    out.push_str(&format!(
        "  {:<18}{}\n",
        "package", "a signed, notarised Flower.app in a .dmg, in target/package/ (--help)"
    ));
    out.push_str(
        "\nCI:         ship ci [<job>]   (the jobs are in .config/ship.figl)\n\
         releasing:  ship <command>    (see `ship help`)\n",
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
// This came across from `release.rs` when releasing left xtask. Only the
// isolation test still asks the question, and it is a CI question: a
// crate added to the workspace and left out of `ISOLATED` is built by
// `--workspace` and by nothing alone.

#[cfg(test)]
/// A workspace member: where its manifest lives, and what the registry calls it.
///
/// flower keeps its crates under `crates/`, so a member's path and its package
/// name are two different strings — `crates/flower-core` is read, `flower-core`
/// is what `cargo check -p` is asked about. Conflating them is how a job ends up
/// looking for a crate named after a directory.
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

    /// Read a workspace file, by its path from the root.
    fn read(&self, path: &str) -> Result<String> {
        let path = self.root.join(path);
        std::fs::read_to_string(&path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))
    }

    /// `workspace.package.version`, the version every crate and the app take.
    fn workspace_version(&self) -> Result<String> {
        let manifest = self.read("Cargo.toml")?;
        let mut in_package = false;
        for line in manifest.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_package = line == "[workspace.package]";
            } else if in_package
                && let Some(rest) = line.strip_prefix("version")
                && let Some(value) = rest.split('"').nth(1)
            {
                return Ok(value.to_owned());
            }
        }
        Err("no `version` under [workspace.package] in Cargo.toml".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            found
                .iter()
                .any(|m| m.path == "crates/flower-core" && m.name == "flower-core")
        );
        for name in ["flower-ffi", "flower-ratatui", "flower-tui", "xtask"] {
            assert!(found.iter().any(|m| m.name == name), "{name} was not read");
        }
        // Nothing goes to crates.io: flower is consumed from git, by commit,
        // and a member that loses its `publish = false` is one a stray
        // `cargo publish` would upload.
        for m in &found {
            assert!(!m.publishable, "{} is not `publish = false`", m.name);
        }
        assert!(found.len() >= 5, "the array spans several lines");
    }
}
