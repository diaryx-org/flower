//! The Apple app in `apps/flower-editor`: building and launching it, and the
//! one version number of its that no manifest parser reaches.
//!
//! `cargo xtask swift` is the chain behind ⌘R, four toolchains deep: cargo
//! builds `crates/flower-ffi`, uniffi-bindgen turns it into Swift, xcodegen
//! turns `project.yml` into an Xcode project, and xcodebuild builds the app
//! (its own pre-build script rebuilding the Rust staticlib). The first two and
//! the third are `apps/flower-editor/bootstrap.sh`'s job and stay there — this
//! decides *when* they need to run, then builds and launches.
//!
//! `cargo xtask sync-versions` writes the workspace version into the app's
//! `MARKETING_VERSION`, and the `app-version` job checks the two agree. The
//! release tooling moves every Cargo manifest and knows nothing about an Xcode
//! setting, so `.config/release.toml` runs the sync as the bump's `post_bump`.

use crate::{Result, Sh};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const APP_DIR: &str = "apps/flower-editor";
/// The scheme, the product and the process are all named this, by
/// `apps/flower-editor/project.yml`.
pub(crate) const SCHEME: &str = "Flower";
const PROJECT_YML: &str = "apps/flower-editor/project.yml";
/// How the version line in [`PROJECT_YML`] begins.
const VERSION_PREFIX: &str = "        MARKETING_VERSION: \"";

/// The file a launch opens when none is given, copied to `target/` so the
/// developer can edit and save it: a document app with nothing to open shows
/// the Open panel at launch.
const SAMPLE: &str = "sample.toml";

const SWIFT_USAGE: &str =
    "usage: cargo xtask swift [--release] [--regen] [--build-only] [--verbose] [FILE]";

/// `cargo xtask swift …`.
pub fn swift(sh: &Sh, args: &[&str]) -> Result<()> {
    let mut release = false;
    let mut regen = false;
    let mut build_only = false;
    let mut verbose = false;
    let mut file = None;
    for &arg in args {
        match arg {
            "--release" => release = true,
            "--regen" => regen = true,
            "--build-only" => build_only = true,
            "--verbose" => verbose = true,
            "-h" | "--help" => {
                println!("{SWIFT_USAGE}");
                return Ok(());
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown flag `{flag}`\n\n{SWIFT_USAGE}"));
            }
            path if file.is_none() => file = Some(PathBuf::from(path)),
            _ => return Err(format!("one file at a time\n\n{SWIFT_USAGE}")),
        }
    }

    let app_dir = sh.root.join(APP_DIR);
    let project = app_dir.join(format!("{SCHEME}.xcodeproj"));
    // The project is git-ignored and regenerable, so a fresh checkout lands
    // here on the first run rather than in an xcodebuild error about a missing
    // project.
    if regen || !project.exists() {
        sh.run("bash", &[&format!("{APP_DIR}/bootstrap.sh")])?;
    }

    let config = if release { "Release" } else { "Debug" };
    let derived = app_dir.join("build/DD");
    let (project, derived) = (path_str(&project)?, path_str(&derived)?);
    let mut build = vec![
        "-project",
        project,
        "-scheme",
        SCHEME,
        "-configuration",
        config,
        "-destination",
        "platform=macOS",
        "-derivedDataPath",
        derived,
        "build",
    ];
    if !verbose {
        build.push("-quiet");
    }
    sh.run("xcodebuild", &build)?;

    let product = app_dir
        .join("build/DD/Build/Products")
        .join(config)
        .join(format!("{SCHEME}.app"));
    if !product.is_dir() {
        return Err(format!(
            "xcodebuild reported success but {} is missing",
            product.display()
        ));
    }
    if build_only {
        println!("built {}", product.display());
        return Ok(());
    }

    let file = match file {
        Some(file) => std::fs::canonicalize(&file)
            .map_err(|e| format!("no such file {}: {e}", file.display()))?,
        None => sample_copy(sh)?,
    };
    // `open` on a bundle that is already running only raises its window, which
    // would silently show the *previous* build. Retiring the old instance first
    // makes "run" mean the thing that was just built.
    let _ = Command::new("pkill").args(["-x", SCHEME]).status();
    // Through the document system, as a double-click in the Finder would.
    sh.run("open", &["-a", path_str(&product)?, path_str(&file)?])
}

/// [`SAMPLE`], copied to `target/flower-sample/`.
fn sample_copy(sh: &Sh) -> Result<PathBuf> {
    let dir = sh.root.join("target/flower-sample");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let copy = dir.join(SAMPLE);
    std::fs::copy(sh.root.join(SAMPLE), &copy)
        .map_err(|e| format!("could not copy {SAMPLE} to {}: {e}", copy.display()))?;
    Ok(copy)
}

pub(crate) fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| format!("{} is not UTF-8", path.display()))
}

/// The CI job: the app states the workspace version.
pub fn check_version(sh: &Sh) -> Result<()> {
    let version = sh.workspace_version()?;
    let (_, current) = app_version(&sh.read(PROJECT_YML)?)?;
    if current == version {
        println!("{PROJECT_YML} is at {version}");
        Ok(())
    } else {
        Err(format!(
            "{PROJECT_YML} says {current} but the workspace is at {version} — \
             run `cargo xtask sync-versions`"
        ))
    }
}

/// `cargo xtask sync-versions`: write the workspace version into the app.
/// Idempotent.
pub fn sync_versions(sh: &Sh) -> Result<()> {
    let version = sh.workspace_version()?;
    let text = sh.read(PROJECT_YML)?;
    let (line, current) = app_version(&text)?;
    if current == version {
        println!("{PROJECT_YML} already at {version}");
        return Ok(());
    }
    let mut out: Vec<String> = text.lines().map(str::to_owned).collect();
    out[line] = format!("{VERSION_PREFIX}{version}\"");
    let mut joined = out.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    let path = sh.root.join(PROJECT_YML);
    std::fs::write(&path, joined)
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
    println!("{PROJECT_YML}: {current} → {version}");
    Ok(())
}

/// The version `project.yml` states, and the line it states it on.
fn app_version(text: &str) -> Result<(usize, String)> {
    text.lines()
        .enumerate()
        .find_map(|(i, line)| {
            let rest = line.strip_prefix(VERSION_PREFIX)?;
            Some((i, rest.split('"').next().unwrap_or_default().to_string()))
        })
        .ok_or_else(|| {
            format!("no version line in {PROJECT_YML} (looked for one starting {VERSION_PREFIX:?})")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_version_line_is_found() {
        let yml = "settings:\n  base:\n        MARKETING_VERSION: \"1.2.3\"\n";
        assert_eq!(app_version(yml).unwrap(), (2, "1.2.3".to_string()));
        assert!(app_version("MARKETING_VERSION: \"1.2.3\"\n").is_err());
    }

    /// The real file, so a reformatted `project.yml` fails here rather than at
    /// release time.
    #[test]
    fn project_yml_states_a_version() {
        let sh = Sh::new();
        app_version(&sh.read(PROJECT_YML).unwrap()).unwrap();
    }
}
