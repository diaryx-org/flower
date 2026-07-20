//! A [`flower_core::Backend`] that edits a prov document's embedded metadata.
//!
//! Where [`flower_core::FigBackend`] drives a standalone config file,
//! [`ProvBackend`] drives the *metadata region* of a prov document — frontmatter
//! in a prose file, or a whole-file config document — through prov's
//! carrier-aware [`MetaEditor`](prov::edit::MetaEditor). Edits are lossless:
//! comments, key order, the carrier/format, and — crucially — the prose body are
//! all preserved; only the changed metadata node's bytes move.
//!
//! - [`Backend::to_value`] returns just the metadata tree (what flower renders).
//! - [`Backend::source`] returns the whole document text (frontmatter + body),
//!   which the embedder writes on save.
//! - [`ProvBackend::body`] exposes the prose body — the region a `leaf` editor
//!   would own. Here it simply rides along untouched, proving the two regions
//!   are independent.
//!
//! Scope: this is the single-document metadata surface (prov's `edit` layer).
//! Relation fields (`contents`/`part_of`/`links`) that maintain inverse links
//! *across* documents belong to prov's `mutate` layer — a later,
//! relationship-aware backend, not this one.

use fig::Value;
use flower_core::tree::to_fig;
use flower_core::{Backend, BackendError, EditOp, Seg};
use prov::edit::MetaEditor;
use prov::{Document, MetaCarrier};

fn be(e: impl std::fmt::Display) -> BackendError {
    BackendError(e.to_string())
}

/// A backend over a single prov document, editing its embedded metadata.
pub struct ProvBackend {
    /// The document path — drives carrier/format detection (extension for a
    /// whole-file config doc, content sniffing for a fenced block).
    path: std::path::PathBuf,
    /// The current full document text (frontmatter + body); the source of truth.
    text: String,
}

impl ProvBackend {
    /// Open a prov document from its full `text`. Errors if prov cannot parse it.
    pub fn open(
        path: impl Into<std::path::PathBuf>,
        text: impl Into<String>,
    ) -> Result<Self, BackendError> {
        let path = path.into();
        let text = text.into();
        // Fail fast if the document doesn't parse.
        Document::parse(&path, &text).map_err(be)?;
        Ok(Self { path, text })
    }

    fn document(&self) -> Result<Document, BackendError> {
        Document::parse(&self.path, &self.text).map_err(be)
    }

    /// The prose body outside the metadata block — the region a `leaf` editor
    /// would own. Empty for a whole-file config document.
    pub fn body(&self) -> Result<String, BackendError> {
        Ok(self.document()?.body)
    }

    /// Replace the prose body, leaving the metadata block untouched — the write
    /// path for edits a `leaf` editor makes to [`body`](Self::body).
    ///
    /// Uses fig's `Embed::replace_body` (the same lossless primitive prov edits
    /// through). A production GUI would route this through prov's write path so
    /// fixity/`updated` restamping fires; here it demonstrates that the metadata
    /// and body regions edit independently over one document.
    pub fn set_body(&mut self, body: &str) -> Result<(), BackendError> {
        match self.document()?.carrier {
            Some(MetaCarrier::Fenced(kind)) => {
                let mut embed = fig::Embed::open(self.text.as_bytes(), kind).map_err(be)?;
                embed.replace_body(body).map_err(be)?;
                self.text = embed.render().map_err(be)?.to_string();
                Ok(())
            }
            _ => Err(BackendError(
                "document has no fenced body to replace".into(),
            )),
        }
    }
}

impl Backend for ProvBackend {
    fn apply(&mut self, op: EditOp) -> Result<(), BackendError> {
        let carrier = self.document()?.carrier;
        // `open_or_init` so an edit to a document with no block synthesizes one
        // (frontmatter for a prose file) rather than failing.
        let mut editor = MetaEditor::open_or_init(&self.text, carrier).map_err(be)?;

        match op {
            EditOp::ReplaceValue { path, value } => {
                let segs = to_fig(&path);
                // Mirror prov's `set_in_text`: an index-terminated path is a pure
                // replacement (there is no "insert at absent index"); a
                // key-terminated path upserts.
                match path.last() {
                    Some(Seg::Index(_)) => editor.replace_value(&segs, value).map_err(be)?,
                    _ => editor.set_value(&segs, value).map_err(be)?,
                }
            }
            EditOp::DeleteKey { path } => editor.delete(&to_fig(&path)).map_err(be)?,
            EditOp::RemoveItem { seq_path, index } => {
                editor.remove_item(&to_fig(&seq_path), index).map_err(be)?
            }
        }

        self.text = editor.render().map_err(be)?;
        Ok(())
    }

    fn to_value(&self) -> Result<Value, BackendError> {
        // prov's metadata tree → fig's value tree (the serde-free bridge).
        Ok(Value::from(&self.document()?.meta))
    }

    fn source(&self) -> Result<String, BackendError> {
        Ok(self.text.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flower_core::{Mode, Model};

    const DOC: &str = "\
---
# the title
title: Old Title
draft: true
tags:
- a
- b
---
# Heading

Body prose that must survive metadata edits.
";

    fn model() -> Model<ProvBackend> {
        let backend = ProvBackend::open("note.md", DOC).expect("open prov doc");
        Model::new(backend).expect("build model")
    }

    fn select(model: &mut Model<ProvBackend>, path: &[Seg]) {
        model.selected = model
            .rows
            .iter()
            .position(|r| r.path == path)
            .unwrap_or_else(|| panic!("no row for {path:?}"));
    }

    fn type_value(model: &mut Model<ProvBackend>, text: &str) {
        if let Mode::Editing { buffer } = &mut model.mode {
            buffer.clear();
        }
        for c in text.chars() {
            model.edit_push(c);
        }
        model.edit_commit();
    }

    #[test]
    fn renders_frontmatter_as_a_tree() {
        let model = model();
        let keys: Vec<&str> = model
            .rows
            .iter()
            .filter(|r| r.depth == 0)
            .map(|r| r.label.as_str())
            .collect();
        assert_eq!(keys, ["title", "draft", "tags"], "top-level frontmatter keys");
    }

    #[test]
    fn edits_metadata_leaving_the_body_untouched() {
        let mut model = model();

        select(&mut model, &[Seg::Key("title".into())]);
        model.begin_edit();
        type_value(&mut model, "New Title");

        let out = model.source_snapshot();
        assert!(out.contains("title: New Title"), "value changed:\n{out}");
        assert!(out.contains("# the title"), "comment preserved:\n{out}");
        assert!(out.starts_with("---\n"), "fences intact:\n{out}");
        assert!(
            out.contains("Body prose that must survive metadata edits."),
            "body preserved:\n{out}"
        );
        // The body region is byte-identical — exactly what leaf would own.
        assert_eq!(
            model.source_snapshot()[out.rfind("# Heading").unwrap()..],
            DOC[DOC.rfind("# Heading").unwrap()..]
        );
    }

    #[test]
    fn deletes_a_key() {
        let mut model = model();

        select(&mut model, &[Seg::Key("draft".into())]);
        model.delete_selected();

        let out = model.source_snapshot();
        assert!(!out.contains("draft:"), "key removed:\n{out}");
        assert!(out.contains("title: Old Title"), "siblings kept:\n{out}");
        assert!(out.contains("Body prose"), "body kept:\n{out}");
    }

    #[test]
    fn removes_a_sequence_item() {
        let mut model = model();

        // Expand `tags`, then delete its second item (`b`).
        select(&mut model, &[Seg::Key("tags".into())]);
        model.expand_or_enter();
        select(
            &mut model,
            &[Seg::Key("tags".into()), Seg::Index(1)],
        );
        model.delete_selected();

        let out = model.source_snapshot();
        assert!(out.contains("- a"), "kept first item:\n{out}");
        assert!(!out.contains("- b"), "removed second item:\n{out}");
    }

    /// The whole composition, headless: flower edits the metadata, leaf edits the
    /// body, both land in one prov document with everything else preserved.
    #[test]
    fn full_round_trip_metadata_via_flower_and_body_via_leaf() {
        use leaf_core::{Doc, Format};

        let mut model = model();

        // 1. Metadata edit, through flower's structural model.
        select(&mut model, &[Seg::Key("title".into())]);
        model.begin_edit();
        type_value(&mut model, "New Title");

        // 2. Body edit, through leaf — a separate editor over just the body
        //    region. leaf owns the body as its own buffer (no shared offsets).
        let body = model.backend().body().expect("body");
        let mut doc = Doc::from_source(body, Format::Markdown).expect("leaf doc");
        doc.insert("Edited: ");
        let new_body = doc.source.clone();

        // 3. Write the edited body back into the same document.
        model.backend_mut().set_body(&new_body).expect("set body");

        // 4. One document now carries both edits; nothing else moved.
        let out = model.source_snapshot();
        assert!(out.contains("title: New Title"), "flower metadata edit:\n{out}");
        assert!(out.contains("# the title"), "frontmatter comment kept:\n{out}");
        assert!(out.contains("Edited: "), "leaf body edit:\n{out}");
        assert!(
            out.contains("Body prose that must survive metadata edits."),
            "rest of body kept:\n{out}"
        );
        assert!(out.starts_with("---\n"), "fences intact:\n{out}");
        // The metadata block and body are disjoint: the body edit left the
        // frontmatter's other keys exactly as flower wrote them.
        assert!(out.contains("draft: true") && out.contains("- a"), "meta intact:\n{out}");
    }
}
