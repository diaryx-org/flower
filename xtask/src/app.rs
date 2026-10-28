//! The Mac app, `Flower.app`: `apps/flower-dioxus` in a bundle, built and
//! launched.
//!
//! The app is a cargo binary, so its bundle is put together here rather than
//! by Xcode, as leaf's is: the binary as `Contents/MacOS/Flower`, and
//! `apps/flower-dioxus/Info.plist` with the workspace's version stamped in —
//! so the version is stated once, in the workspace manifest, and a release
//! bump has nothing else to move. The bundle is signed ad hoc, which is what
//! lets a Mac run it; `cargo xtask package` signs it again for others.
//!
//! `FLOWER_BUNDLE_ID` names another bundle identifier, so a build under test
//! keeps its own preferences and recents apart from the copy of Flower the
//! Mac has installed.

use crate::{Result, Sh};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The app's directory, the cargo workspace it is.
pub(crate) const APP_DIR: &str = "apps/flower-dioxus";
/// The bundle, its executable and its process are all named this.
pub(crate) const APP: &str = "Flower";
/// The binary cargo builds, which the bundle names [`APP`].
const BINARY: &str = "flower-dioxus";
/// The target the bundle is built for: Apple Silicon.
pub(crate) const TARGET: &str = "aarch64-apple-darwin";

/// The file a launch opens when none is given, copied to `target/` so the
/// developer can edit and save it.
const SAMPLE: &str = "sample.toml";

const MAC_USAGE: &str = "usage: cargo xtask mac [--release] [--build-only] [FILE]";

/// `cargo xtask mac …`.
pub fn mac(sh: &Sh, args: &[&str]) -> Result<()> {
    let mut release = false;
    let mut build_only = false;
    let mut file = None;
    for &arg in args {
        match arg {
            "--release" => release = true,
            "--build-only" => build_only = true,
            "-h" | "--help" => {
                println!("{MAC_USAGE}");
                return Ok(());
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown flag `{flag}`\n\n{MAC_USAGE}"));
            }
            path if file.is_none() => file = Some(PathBuf::from(path)),
            _ => return Err(format!("one file at a time\n\n{MAC_USAGE}")),
        }
    }
    let app = bundle(sh, release)?;
    if build_only {
        println!("✓ Built {}", app.display());
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
    let _ = Command::new("pkill").args(["-x", APP]).status();
    // Through the Finder's route, an Apple Event, as a double-click would.
    sh.run("open", &["-a", path_str(&app)?, path_str(&file)?])?;
    println!("✓ Running {APP}, with {}", file.display());
    Ok(())
}

/// Build the binary and put `Flower.app` together around it, in
/// `apps/flower-dioxus/target/<target>/<profile>/`. Answers the bundle's
/// path.
pub fn bundle(sh: &Sh, release: bool) -> Result<PathBuf> {
    let manifest = format!("{APP_DIR}/Cargo.toml");
    let mut build = vec!["build", "--manifest-path", &manifest, "--target", TARGET];
    if release {
        build.push("--release");
    }
    sh.cargo(&build)?;

    let profile = if release { "release" } else { "debug" };
    let out = sh
        .root
        .join(APP_DIR)
        .join("target")
        .join(TARGET)
        .join(profile);
    let app = out.join(format!("{APP}.app"));
    if app.exists() {
        std::fs::remove_dir_all(&app)
            .map_err(|e| format!("could not clear {}: {e}", app.display()))?;
    }
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    std::fs::create_dir_all(&macos)
        .map_err(|e| format!("could not create {}: {e}", macos.display()))?;
    std::fs::create_dir_all(contents.join("Resources"))
        .map_err(|e| format!("could not create the bundle's Resources: {e}"))?;
    std::fs::copy(out.join(BINARY), macos.join(APP))
        .map_err(|e| format!("could not copy the built binary: {e}"))?;

    let plist = contents.join("Info.plist");
    let source = sh.read(&format!("{APP_DIR}/Info.plist"))?;
    let version = sh.workspace_version()?;
    let mut stamped = stamp_version(&source, &version)?;
    if let Ok(id) = std::env::var("FLOWER_BUNDLE_ID") {
        stamped = stamp_key(&stamped, "CFBundleIdentifier", &id)?;
    }
    std::fs::write(&plist, stamped)
        .map_err(|e| format!("could not write {}: {e}", plist.display()))?;
    std::fs::write(contents.join("PkgInfo"), "APPL????")
        .map_err(|e| format!("could not write the bundle's PkgInfo: {e}"))?;

    sh.run("codesign", &["--force", "--sign", "-", path_str(&app)?])?;
    Ok(app)
}

/// `Info.plist` with `version` as the bundle's short version.
pub(crate) fn stamp_version(plist: &str, version: &str) -> Result<String> {
    stamp_key(plist, "CFBundleShortVersionString", version)
}

/// `plist` with `key`'s string value replaced by `value`: the `<string>`
/// on the line after the key's, as the file is laid out.
fn stamp_key(plist: &str, key: &str, value: &str) -> Result<String> {
    let mut lines: Vec<String> = plist.lines().map(str::to_owned).collect();
    let at = lines
        .iter()
        .position(|l| l.trim() == format!("<key>{key}</key>"))
        .ok_or_else(|| format!("no {key} in {APP_DIR}/Info.plist"))?;
    let next = lines
        .get_mut(at + 1)
        .filter(|l| l.trim().starts_with("<string>"))
        .ok_or_else(|| format!("{key} in {APP_DIR}/Info.plist is not followed by a <string>"))?;
    let indent: String = next.chars().take_while(|c| c.is_whitespace()).collect();
    *next = format!("{indent}<string>{value}</string>");
    let mut out = lines.join("\n");
    if plist.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_stamped_on_the_line_after_its_key() {
        let plist =
            "<dict>\n\t<key>CFBundleShortVersionString</key>\n\t<string>0.0.0</string>\n</dict>\n";
        let out = stamp_version(plist, "1.2.3").unwrap();
        assert_eq!(
            out,
            "<dict>\n\t<key>CFBundleShortVersionString</key>\n\t<string>1.2.3</string>\n</dict>\n"
        );
        assert!(stamp_version("<dict></dict>", "1.2.3").is_err());
    }

    /// The real file, so a reformatted `Info.plist` fails here rather than
    /// at release time.
    #[test]
    fn the_apps_info_plist_takes_a_version_and_an_identifier() {
        let sh = Sh::new();
        let plist = sh.read(&format!("{APP_DIR}/Info.plist")).unwrap();
        let out = stamp_version(&plist, "9.9.9").unwrap();
        assert!(out.contains("<string>9.9.9</string>"));
        let out = stamp_key(&out, "CFBundleIdentifier", "org.diaryx.flower-test").unwrap();
        assert!(out.contains("<string>org.diaryx.flower-test</string>"));
        assert!(out.contains(&format!("<string>{APP}</string>")));
    }
}
