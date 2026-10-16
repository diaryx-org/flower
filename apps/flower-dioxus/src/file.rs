//! The file: reading one into a model, and the dialogs that name one.
//!
//! The dialogs are rfd's *async* ones, which on macOS are sheets on the
//! window. A modal dialog would spin a nested event loop from inside a Dioxus
//! task while Blitz holds its document borrowed — the "RefCell already
//! borrowed" panic leaf-dioxus's `file.rs` describes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow_free::Result;
use dioxus_native::winit::window::Window;
use flower_core::{FigBackend, Model};
use rfd::{AsyncFileDialog, AsyncMessageDialog, MessageButtons, MessageDialogResult, MessageLevel};

/// A tiny `Result` with a message, so the app needs no error crate.
mod anyhow_free {
    pub type Result<T> = std::result::Result<T, String>;
}

/// The extensions flower opens: the fig language crates flower-core depends on.
const EXTENSIONS: &[&str] = &[
    "toml", "yaml", "yml", "json", "jsonc", "json5", "zon", "figl", "fig",
];

/// The flower sample, opened when no file is named or the one named would
/// not open.
const SAMPLE: &str = include_str!("../../../sample.toml");

/// The document as a model, read from `path`.
pub fn read(path: &Path) -> Result<Model<FigBackend>> {
    let format = flower_core::detect(path).ok_or_else(|| {
        format!(
            "{}: not a format flower opens (want {})",
            path.display(),
            EXTENSIONS.join(", ")
        )
    })?;
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let backend =
        FigBackend::open(&bytes, format).map_err(|e| format!("{}: {e}", path.display()))?;
    Model::new(backend).map_err(|e| format!("{}: {e}", path.display()))
}

/// The file named, or the sample, untitled, when none was or it would not
/// open — with what went wrong in the status line.
pub fn open_or_sample(path: Option<PathBuf>) -> (Model<FigBackend>, Option<PathBuf>) {
    let mut failed = None;
    if let Some(p) = path {
        match read(&p) {
            Ok(m) => return (m, Some(p)),
            Err(e) => {
                eprintln!("{e}");
                failed = Some(e);
            }
        }
    }
    let format = flower_core::format::from_extension("toml").expect("toml");
    let backend = FigBackend::open(SAMPLE.as_bytes(), format).expect("the sample parses");
    let mut model = Model::new(backend).expect("the sample models");
    if let Some(e) = failed {
        model.set_status(e);
    }
    (model, None)
}

/// Write `model` to `path`, and tell it how that went.
pub fn write(model: &mut Model<FigBackend>, path: &Path) -> bool {
    match std::fs::write(path, model.source_snapshot()) {
        Ok(()) => {
            model.mark_saved();
            model.set_status(format!("saved {}", name_of(Some(path))));
            true
        }
        Err(e) => {
            model.set_status(format!("save failed: {e}"));
            false
        }
    }
}

/// What a window calls a document: its file's name, or "Untitled".
pub fn name_of(path: Option<&Path>) -> String {
    path.and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into())
}

/// Ask for a config to open.
pub async fn pick_open(window: &Arc<dyn Window>) -> Option<PathBuf> {
    let file = AsyncFileDialog::new()
        .add_filter("Config", EXTENSIONS)
        .set_parent(&**window)
        .pick_file()
        .await?;
    Some(file.path().to_path_buf())
}

/// Ask where to save, suggesting `name`.
pub async fn pick_save(window: &Arc<dyn Window>, name: &str) -> Option<PathBuf> {
    let file = AsyncFileDialog::new()
        .set_file_name(name)
        .set_parent(&**window)
        .save_file()
        .await?;
    Some(file.path().to_path_buf())
}

/// What to do with unsaved changes before the document is replaced.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unsaved {
    Save,
    Discard,
    Cancel,
}

/// The Mac's question, in its words.
pub async fn ask_unsaved(window: &Arc<dyn Window>, name: &str) -> Unsaved {
    let answer = AsyncMessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title(format!(
            "Do you want to save the changes you made to “{name}”?"
        ))
        .set_description("Your changes will be lost if you don’t save them.")
        .set_buttons(MessageButtons::YesNoCancelCustom(
            "Save".into(),
            "Don’t Save".into(),
            "Cancel".into(),
        ))
        .set_parent(&**window)
        .show()
        .await;
    match answer {
        MessageDialogResult::Yes => Unsaved::Save,
        MessageDialogResult::No => Unsaved::Discard,
        MessageDialogResult::Custom(label) if label == "Save" => Unsaved::Save,
        MessageDialogResult::Custom(label) if label.starts_with("Don") => Unsaved::Discard,
        _ => Unsaved::Cancel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_opens_untitled_and_a_bad_path_says_why() {
        let (m, path) = open_or_sample(None);
        assert!(path.is_none());
        assert!(m.source_snapshot().contains("flower"));
        let (m, path) = open_or_sample(Some(PathBuf::from("/nonexistent/x.toml")));
        assert!(path.is_none());
        assert!(m.status.contains("/nonexistent/x.toml"), "{}", m.status);
        let (m, _) = open_or_sample(Some(PathBuf::from("notes.txt")));
        assert!(
            m.status.contains("not a format flower opens"),
            "{}",
            m.status
        );
    }

    #[test]
    fn a_save_writes_the_edited_bytes_and_marks_the_model_clean() {
        let dir = std::env::temp_dir().join(format!("flower-dioxus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.toml");
        std::fs::write(&path, "# kept\nport = 1\n").unwrap();
        let mut m = read(&path).unwrap();
        m.set_scalar_text(&[flower_core::Seg::Key("port".into())], "2");
        assert!(m.dirty);
        assert!(write(&mut m, &path));
        assert!(!m.dirty);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# kept\nport = 2\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
