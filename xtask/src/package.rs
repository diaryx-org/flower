//! `cargo xtask package` — Flower for Macs other than the one that built it,
//! outside the App Store: a Developer ID–signed, notarised and stapled
//! `Flower.app` inside a signed, notarised and stapled `.dmg`, written to
//! `target/package/`. Thorn's and Leaf's, ported to this crate's no-dependency
//! idiom.
//!
//! `project.yml` leaves the Mac's Debug build unsigned, so that `cargo xtask
//! swift` works on a machine with no Apple team, and signs its Release build
//! automatically, for the App Store. Developer ID signing is switched on here,
//! from the command line, and nowhere else: xcodebuild's command-line settings
//! outrank the project's, SDK-conditional ones included. The sandbox is the
//! project's and stays on — its entitlements come from build settings and need
//! no provisioning profile — so the app in the image is the App Store's app,
//! signed by another certificate.
//!
//! Two notarisations, not one. A ticket is looked up by the hash of what it
//! covers, so the app is notarised and stapled before it goes into the image —
//! a copy dragged out of the `.dmg` then opens offline — and the image, whose
//! hash stapling the app has just changed, is notarised in its own right.
//!
//! What this needs from the machine:
//!
//! - a `Developer ID Application` identity for the team in the keychain. The
//!   team is the project's own unless `DEVELOPMENT_TEAM` names another.
//! - notary credentials, either `NOTARY_KEYCHAIN_PROFILE` (a profile stored
//!   with `xcrun notarytool store-credentials`) or an App Store Connect API
//!   key as `APPLE_API_KEY_PATH`, `APPLE_API_KEY_ID` and `APPLE_API_ISSUER_ID`
//!   — the names diaryx's release workflow already gives its secrets.
//!   `--no-notarize` skips both notarisations and needs neither.
//!
//! `.github/workflows/mac-app.yml` runs this on a release tag, from secrets.
//!
//! Apple Silicon only, for now: the project's pre-build script builds the Rust
//! staticlib for `aarch64-apple-darwin` alone, and the file name says so.

use crate::app::{APP_DIR, SCHEME, path_str};
use crate::{Result, Sh};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The team `DEVELOPMENT_TEAM` in `apps/flower-editor/project.yml` names.
const TEAM: &str = "V4322HH5HU";

/// The entitlements `ENABLE_APP_SANDBOX` and `ENABLE_USER_SELECTED_FILES`
/// write for Release in `apps/flower-editor/project.yml`, which the signed app
/// must still carry.
const SANDBOX: [&str; 2] = [
    "com.apple.security.app-sandbox",
    "com.apple.security.files.user-selected.read-write",
];

const USAGE: &str = "usage: cargo xtask package [--no-notarize] [--verbose]";

/// `cargo xtask package …`.
pub fn package(sh: &Sh, args: &[&str]) -> Result<()> {
    // Sign, but do not notarise: an image for checking the build, which
    // Gatekeeper on another Mac will still refuse.
    let mut no_notarize = false;
    let mut verbose = false;
    for &arg in args {
        match arg {
            "--no-notarize" => no_notarize = true,
            "--verbose" => verbose = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown argument `{other}`\n\n{USAGE}")),
        }
    }

    let team = std::env::var("DEVELOPMENT_TEAM").unwrap_or_else(|_| TEAM.to_string());
    let identity = identity(&team)?;
    // Asked before a build that takes minutes, not after it.
    let notary = if no_notarize {
        None
    } else {
        Some(Notary::from_env()?)
    };

    // Always regenerated: the project is git-ignored, and one made before a
    // source file was added builds without it — or, from an older
    // `project.yml`, for the wrong deployment target — and says nothing.
    sh.run("bash", &[&format!("{APP_DIR}/bootstrap.sh")])?;
    let app_dir = sh.root.join(APP_DIR);
    let project = app_dir.join(format!("{SCHEME}.xcodeproj"));
    // Its own derived data: a signed and an unsigned build of the same
    // configuration would otherwise take turns invalidating each other.
    let derived = app_dir.join("build/DD-package");

    let identity_setting = format!("CODE_SIGN_IDENTITY={}", identity);
    let team_setting = format!("DEVELOPMENT_TEAM={team}");
    let mut build = vec![
        "-project",
        path_str(&project)?,
        "-scheme",
        SCHEME,
        "-configuration",
        "Release",
        "-destination",
        "platform=macOS",
        "-derivedDataPath",
        path_str(&derived)?,
        "clean",
        "build",
        "CODE_SIGNING_ALLOWED=YES",
        "CODE_SIGNING_REQUIRED=YES",
        "CODE_SIGN_STYLE=Manual",
        "PROVISIONING_PROFILE_SPECIFIER=",
        // Notarisation refuses code without the hardened runtime.
        // `get-task-allow`, which it refuses too, is taken out after the build
        // by `without_get_task_allow`, not here: turning off
        // CODE_SIGN_INJECT_BASE_ENTITLEMENTS drops the sandbox entitlements
        // along with it.
        "ENABLE_HARDENED_RUNTIME=YES",
        "OTHER_CODE_SIGN_FLAGS=--timestamp",
        &identity_setting,
        &team_setting,
    ];
    if !verbose {
        build.push("-quiet");
    }
    sh.run("xcodebuild", &build)?;

    let built = derived.join(format!("Build/Products/Release/{SCHEME}.app"));
    if !built.is_dir() {
        return Err(format!(
            "xcodebuild reported success but {} is missing",
            built.display()
        ));
    }

    let out = sh.root.join("target/package");
    if out.exists() {
        std::fs::remove_dir_all(&out)
            .map_err(|e| format!("could not clear {}: {e}", out.display()))?;
    }
    std::fs::create_dir_all(&out)
        .map_err(|e| format!("could not create {}: {e}", out.display()))?;
    let app = out.join(format!("{SCHEME}.app"));
    // `ditto`, not a plain copy: it keeps the bundle's symlinks, extended
    // attributes and signature intact.
    sh.run("ditto", &[path_str(&built)?, path_str(&app)?])?;
    without_get_task_allow(sh, &app, &identity, &out)?;
    verify_signature(sh, &app)?;

    if let Some(notary) = &notary {
        let zip = out.join(format!("{SCHEME}.zip"));
        sh.run(
            "ditto",
            &["-c", "-k", "--keepParent", path_str(&app)?, path_str(&zip)?],
        )?;
        notary.submit(&zip)?;
        remove(&zip)?;
        sh.run("xcrun", &["stapler", "staple", path_str(&app)?])?;
        sh.run(
            "spctl",
            &["--assess", "--type", "execute", "-vv", path_str(&app)?],
        )?;
    }

    let plist = app.join("Contents/Info.plist");
    let version = stdout(
        Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", "Print :CFBundleShortVersionString"])
            .arg(&plist),
    )?;
    let dmg = out.join(format!("{SCHEME}-{}-aarch64.dmg", version.trim()));
    image(sh, &app, &dmg)?;
    sh.run(
        "codesign",
        &["--sign", &identity, "--timestamp", path_str(&dmg)?],
    )?;

    match &notary {
        Some(notary) => {
            notary.submit(&dmg)?;
            sh.run("xcrun", &["stapler", "staple", path_str(&dmg)?])?;
            sh.run(
                "spctl",
                &[
                    "--assess",
                    "--type",
                    "open",
                    "--context",
                    "context:primary-signature",
                    "-vv",
                    path_str(&dmg)?,
                ],
            )?;
            println!("✓ Signed, notarised and stapled {}", dmg.display());
        }
        None => println!(
            "✓ Signed {} (not notarised: another Mac will refuse it)",
            dmg.display()
        ),
    }
    Ok(())
}

/// The SHA-1 of the team's `Developer ID Application` identity. The hash is
/// what gets passed on, not the name: a certificate imported into more than
/// one keychain lists once per keychain, and codesign calls a name that
/// matches twice ambiguous.
fn identity(team: &str) -> Result<String> {
    let listing =
        stdout(Command::new("security").args(["find-identity", "-v", "-p", "codesigning"]))?;
    // `  1) <SHA-1> "Developer ID Application: <name> (<team>)"`
    let wanted = format!("({team})\"");
    listing
        .lines()
        .find_map(|line| {
            let (_, rest) = line.trim().split_once(") ")?;
            let (hash, name) = rest.split_once(' ')?;
            (name.starts_with("\"Developer ID Application:") && name.ends_with(&wanted))
                .then(|| hash.to_string())
        })
        .ok_or_else(|| {
            format!(
                "no `Developer ID Application` identity for team {team} in the keychain — \
                 create one at developer.apple.com (Certificates, Identifiers & Profiles) \
                 or set DEVELOPMENT_TEAM to the team that has one"
            )
        })
}

/// Re-sign `app` with the entitlements Xcode gave it, less the
/// `get-task-allow` it adds for a debugger to attach. Xcode leaves that out of
/// an archive exported for Developer ID; a plain build keeps it. The bundle
/// has one executable and no nested code, so signing the bundle is the whole
/// of it.
fn without_get_task_allow(sh: &Sh, app: &Path, identity: &str, scratch: &Path) -> Result<()> {
    let entitlements = scratch.join("entitlements.plist");
    let (app, plist) = (path_str(app)?, path_str(&entitlements)?);
    sh.run(
        "codesign",
        &["--display", "--xml", "--entitlements", plist, app],
    )?;
    // A missing key is a failure for PlistBuddy, and a fine outcome here.
    let _ = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Delete :com.apple.security.get-task-allow", plist])
        .status();
    sh.run(
        "codesign",
        &[
            "--force",
            "--sign",
            identity,
            "--options",
            "runtime",
            "--timestamp",
            "--entitlements",
            plist,
            app,
        ],
    )?;
    remove(&entitlements)
}

/// A strict check of the signature, and of what notarisation and the
/// project's sandbox would otherwise lose without a word: the hardened
/// runtime, no `get-task-allow`, and the sandbox entitlements project.yml
/// sets for Release.
fn verify_signature(sh: &Sh, app: &Path) -> Result<()> {
    let shown = app.display();
    let app = path_str(app)?;
    sh.run(
        "codesign",
        &["--verify", "--deep", "--strict", "--verbose=2", app],
    )?;
    // `codesign --display` writes its report to stderr.
    let report = Command::new("codesign")
        .args(["--display", "--verbose=2", app])
        .output()
        .map_err(|e| format!("could not run `codesign`: {e}"))?;
    let report = String::from_utf8_lossy(&report.stderr);
    if !report
        .lines()
        .any(|l| l.contains("flags=") && l.contains("runtime"))
    {
        return Err(format!(
            "{shown} is signed without the hardened runtime, which notarisation refuses:\n{report}"
        ));
    }
    let entitlements =
        stdout(Command::new("codesign").args(["--display", "--xml", "--entitlements", "-", app]))?;
    if entitlements.contains("com.apple.security.get-task-allow") {
        return Err(format!(
            "{shown} is signed with get-task-allow, which notarisation refuses"
        ));
    }
    for wanted in SANDBOX {
        if !entitlements.contains(wanted) {
            return Err(format!(
                "{shown} is signed without {wanted}, which project.yml's Release build \
                 sets; the app would ship unsandboxed:\n{entitlements}"
            ));
        }
    }
    Ok(())
}

/// The disk image: the app and a link to `/Applications` to drag it onto.
fn image(sh: &Sh, app: &Path, dmg: &Path) -> Result<()> {
    let staging = dmg.with_extension("staging");
    std::fs::create_dir_all(&staging)
        .map_err(|e| format!("could not create {}: {e}", staging.display()))?;
    let staged = staging.join(format!("{SCHEME}.app"));
    sh.run("ditto", &[path_str(app)?, path_str(&staged)?])?;
    std::os::unix::fs::symlink("/Applications", staging.join("Applications"))
        .map_err(|e| format!("could not link /Applications into the image: {e}"))?;
    sh.run(
        "hdiutil",
        &[
            "create",
            "-volname",
            SCHEME,
            "-fs",
            "HFS+",
            "-format",
            "UDZO",
            "-ov",
            "-srcfolder",
            path_str(&staging)?,
            path_str(dmg)?,
        ],
    )?;
    std::fs::remove_dir_all(&staging)
        .map_err(|e| format!("could not remove {}: {e}", staging.display()))
}

/// How `notarytool` is to authenticate.
enum Notary {
    Profile(String),
    ApiKey {
        path: PathBuf,
        id: String,
        issuer: String,
    },
}

impl Notary {
    fn from_env() -> Result<Self> {
        let var = |name| std::env::var(name).ok().filter(|v: &String| !v.is_empty());
        if let Some(profile) = var("NOTARY_KEYCHAIN_PROFILE") {
            return Ok(Notary::Profile(profile));
        }
        match (
            var("APPLE_API_KEY_PATH"),
            var("APPLE_API_KEY_ID"),
            var("APPLE_API_ISSUER_ID"),
        ) {
            (Some(path), Some(id), Some(issuer)) => Ok(Notary::ApiKey {
                path: path.into(),
                id,
                issuer,
            }),
            _ => Err(
                "no notary credentials: set NOTARY_KEYCHAIN_PROFILE to a profile made \
                 with `xcrun notarytool store-credentials`, or APPLE_API_KEY_PATH, \
                 APPLE_API_KEY_ID and APPLE_API_ISSUER_ID to an App Store Connect API key \
                 — or pass --no-notarize for a signed image only"
                    .into(),
            ),
        }
    }

    fn auth(&self, command: &mut Command) {
        match self {
            Notary::Profile(profile) => {
                command.args(["--keychain-profile", profile]);
            }
            Notary::ApiKey { path, id, issuer } => {
                command
                    .arg("--key")
                    .arg(path)
                    .args(["--key-id", id, "--issuer", issuer]);
            }
        }
    }

    /// Upload `file` and wait for Apple's verdict. `--wait` returns once the
    /// submission settles whatever the verdict, so the status is read from the
    /// output, and a rejection fetches the log that says why.
    fn submit(&self, file: &Path) -> Result<()> {
        println!(
            "\x1b[2m$ xcrun notarytool submit {} --wait\x1b[0m",
            file.display()
        );
        let mut submit = Command::new("xcrun");
        submit.args(["notarytool", "submit"]).arg(file);
        self.auth(&mut submit);
        submit.args(["--wait", "--output-format", "json"]);
        let json = stdout(&mut submit)?;
        if json_field(&json, "status").as_deref() == Some("Accepted") {
            println!("✓ Notarised {}", file.display());
            return Ok(());
        }
        if let Some(id) = json_field(&json, "id") {
            let mut log = Command::new("xcrun");
            log.args(["notarytool", "log", &id]);
            self.auth(&mut log);
            if let Ok(log) = stdout(&mut log) {
                eprintln!("{log}");
            }
        }
        Err(format!(
            "notarisation of {} was not accepted: {json}",
            file.display()
        ))
    }
}

/// Run `command` and return what it printed, failing if it failed.
fn stdout(command: &mut Command) -> Result<String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command
        .output()
        .map_err(|e| format!("could not run `{program}`: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "`{program}` failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("`{program}` printed non-UTF-8"))
}

fn remove(file: &Path) -> Result<()> {
    std::fs::remove_file(file).map_err(|e| format!("could not remove {}: {e}", file.display()))
}

/// A top-level string field of notarytool's one-line JSON. Enough for the
/// three keys it prints — `id`, `message`, `status` — without a JSON crate
/// for a task runner that otherwise has no dependencies at all.
fn json_field(json: &str, key: &str) -> Option<String> {
    let start = json.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = json[start..].trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

#[cfg(test)]
mod tests {
    use super::json_field;

    #[test]
    fn reads_notarytool_output() {
        let json = r#"{"message":"Processing complete","id":"2efe2717-52ef-43a5-96dc-0797e4ca1041","status":"Invalid"}"#;
        assert_eq!(json_field(json, "status").as_deref(), Some("Invalid"));
        assert_eq!(
            json_field(json, "id").as_deref(),
            Some("2efe2717-52ef-43a5-96dc-0797e4ca1041")
        );
        assert_eq!(json_field(json, "missing"), None);
    }
}
