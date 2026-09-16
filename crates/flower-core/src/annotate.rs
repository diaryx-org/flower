//! Per-row findings: what the *host* has to say about a node, drawn beside it.
//!
//! flower-core validates a value against a [`Schema`](crate::Schema) at the
//! commit funnel, which answers "may this be written?" one edit at a time. An
//! [`Annotation`] answers the other question — "what is wrong with the document
//! as it stands?" — and flower cannot answer it: a broken link, a duplicate id,
//! a containment cycle are facts about a *workspace*, and flower-core is
//! single-document and has no filesystem. So the host computes them and hands
//! them over, the way it hands over hidden keys, demoted keys, and a schema.
//!
//! They are host state, not document state. Nothing here is written to the
//! file, nothing here survives a reopen, and an edit does not clear them: the
//! model re-attaches whatever it was last given on every rebuild, so a row
//! keeps its marker while the reader types. What an edit *does* invalidate is
//! whether the finding is still true, and only the host can re-run the check
//! that decided — so a host refreshes them after a save (or whenever its check
//! finishes) by calling [`Model::set_annotations`](crate::Model::set_annotations)
//! again.

use crate::tree::Seg;

/// How loudly a finding reads.
///
/// Deliberately three, and deliberately not a number: a host with five levels
/// maps them down, and a renderer with one glyph per level never has to guess
/// where the cut is.
///
/// Named `Severity` inside this module and not re-exported at the crate root,
/// where the name is already fig-schema's
/// [`Severity`](fig_schema::Severity) — a different fact, about what *changing*
/// a field costs rather than about what is wrong with it now. Two things worth
/// telling apart are worth two names, and `annotate::Severity` is the one that
/// moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    /// The document is wrong: a link that resolves to nothing, a required
    /// field that is absent.
    Error,
    /// The document is suspicious: a retired term, a relation with no inverse.
    Warning,
    /// Something worth knowing and nothing to fix.
    Info,
}

/// One host finding, addressed at a node by the same path everything else here
/// is addressed by.
///
/// The path need not name a row that exists. A finding about a key the document
/// has since lost is simply never matched — dropping it would make the host
/// responsible for pruning its own list against a tree it does not own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Annotation {
    /// The fig path of the node this is about. The empty path is the document.
    pub path: Vec<Seg>,
    pub severity: Severity,
    /// One line, written for the person looking at the row. A renderer with a
    /// status bar shows it there; one with room shows it under the row.
    pub message: String,
}

impl Annotation {
    /// A finding at `path`.
    pub fn new(path: Vec<Seg>, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            path,
            severity,
            message: message.into(),
        }
    }

    /// Shorthand for an [`Severity::Error`] at `path`.
    pub fn error(path: Vec<Seg>, message: impl Into<String>) -> Self {
        Self::new(path, Severity::Error, message)
    }

    /// Shorthand for a [`Severity::Warning`] at `path`.
    pub fn warning(path: Vec<Seg>, message: impl Into<String>) -> Self {
        Self::new(path, Severity::Warning, message)
    }

    /// Shorthand for an [`Severity::Info`] at `path`.
    pub fn info(path: Vec<Seg>, message: impl Into<String>) -> Self {
        Self::new(path, Severity::Info, message)
    }
}

/// The finding that applies at `path`: the one addressed exactly at it, else
/// the one addressed at its nearest annotated ancestor.
///
/// The fallback is what makes a finding about a list answer for a question
/// asked about an item of it — a host inspecting `contents.3` and finding
/// nothing there wants to know that `contents` is in trouble. It is *not* how
/// rows are marked ([`Model::annotation_at`](crate::Model::annotation_at)
/// versus what [`build_page`](crate::page::build_page)'s rows carry): a marker
/// inherited down a subtree would put an error glyph on ninety-five rows
/// because one of them was wrong, and the row that is wrong is the one worth
/// pointing at.
///
/// Among equals — two findings at the same path — the first given wins, so a
/// host's own ordering decides.
pub fn applying_at<'a>(annotations: &'a [Annotation], path: &[Seg]) -> Option<&'a Annotation> {
    if let Some(exact) = annotations.iter().find(|a| a.path == path) {
        return Some(exact);
    }
    annotations
        .iter()
        .filter(|a| a.path.len() < path.len() && path.starts_with(&a.path))
        .max_by_key(|a| a.path.len())
}

/// The finding addressed exactly at `path` — what a row carries.
pub fn exactly_at<'a>(annotations: &'a [Annotation], path: &[Seg]) -> Option<&'a Annotation> {
    annotations.iter().find(|a| a.path == path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str) -> Seg {
        Seg::Key(k.to_string())
    }

    #[test]
    fn an_exact_finding_beats_an_ancestors() {
        let annotations = vec![
            Annotation::warning(vec![key("contents")], "two of these are missing"),
            Annotation::error(vec![key("contents"), Seg::Index(3)], "resolves to nothing"),
        ];
        let item = [key("contents"), Seg::Index(3)];
        assert_eq!(
            applying_at(&annotations, &item).map(|a| a.severity),
            Some(Severity::Error)
        );
        // An item with no finding of its own inherits the list's, for a caller
        // asking — and carries none of its own, for a renderer marking.
        let other = [key("contents"), Seg::Index(0)];
        assert_eq!(
            applying_at(&annotations, &other).map(|a| a.severity),
            Some(Severity::Warning)
        );
        assert_eq!(exactly_at(&annotations, &other), None);
        assert!(applying_at(&annotations, &[key("title")]).is_none());
    }

    #[test]
    fn the_nearest_ancestor_is_the_one_that_answers() {
        let annotations = vec![
            Annotation::info(Vec::new(), "the document"),
            Annotation::warning(vec![key("a")], "the outer"),
            Annotation::error(vec![key("a"), key("b")], "the inner"),
        ];
        let deep = [key("a"), key("b"), key("c")];
        assert_eq!(
            applying_at(&annotations, &deep).map(|a| a.message.as_str()),
            Some("the inner")
        );
    }
}
