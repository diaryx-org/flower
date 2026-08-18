//! The commit sink: what [`Model`](crate::Model) edits *through*.
//!
//! `Model` never talks to a concrete editor. It builds path-addressed [`EditOp`]s
//! and hands them to a [`Backend`], and reads the current tree back via
//! [`Backend::to_value`]. That indirection is the integration seam:
//!
//! - [`FigBackend`] drives a raw [`fig::Editor`] — a standalone config file.
//! - A future prov backend will drive prov's frontmatter editor, applying the
//!   same ops but *also* maintaining inverse links, fixity, and the journal —
//!   so the GUI gets those invariants for free without `Model` knowing about
//!   them.
//!
//! Every op is atomic: on error the document is left exactly as it was (fig
//! reparses and rolls back; a prov backend declines and stages nothing).
//!
//! Because a second implementation is where a one-implementation "contract"
//! quietly forks, [`EditOp`]'s guarantees are written down per variant below, and
//! [`conformance`] is a suite any implementation can run against them.

use fig::{Format, Value};

use crate::tree::{self, Seg};

/// One path-addressed edit. The vocabulary grows as `Model` gains operations
/// (insert, reorder, comments, …); today it covers what the editor issues.
///
/// # Contract
///
/// These hold for every variant, and [`conformance::check`] tests them:
///
/// - **Atomic.** On `Err` the document is byte-for-byte what it was.
/// - **Addressed by the pre-edit tree.** Every path and index is resolved against
///   the document as it stands *before* the op — an empty path is the root.
/// - **Local.** Nothing outside the addressed node changes: sibling values,
///   sibling order, comments, and formatting all survive.
/// - **Unresolvable is an error, not a guess.** A path that doesn't resolve, or
///   that names the wrong kind of node (a key step into a sequence, an index into
///   a mapping), is an `Err` — with the two documented exceptions below.
///
/// Two cases are deliberately **unspecified**, because backends differ on them and
/// `Model` never relies on either: what [`ReplaceValue`](Self::ReplaceValue) does
/// at an absent path, and what [`InsertKey`](Self::InsertKey) does at a key that
/// already exists. A backend over an upserting editor collapses both onto "write
/// it anyway"; one over a stricter editor errors. A caller that wants a value to
/// exist regardless must therefore not lean on the coincidence — it should read
/// the tree first ([`tree::value_at`]) and pick the op that fits.
#[derive(Debug, Clone)]
pub enum EditOp {
    /// Replace the scalar/subtree at `path` with `value`, in place: the node keeps
    /// its position among its siblings and its key (or index).
    ///
    /// `path` must resolve to an existing node. **Unspecified** for an absent path
    /// — an upserting backend creates it, a strict one errors. Use
    /// [`InsertKey`](Self::InsertKey) or [`AppendItem`](Self::AppendItem) to
    /// create.
    ReplaceValue { path: Vec<Seg>, value: Value },
    /// Delete the mapping entry at `path`, closing the gap in its parent's key
    /// order. `path` must end in a [`Seg::Key`] and must resolve; use
    /// [`RemoveItem`](Self::RemoveItem) for a sequence item.
    DeleteKey { path: Vec<Seg> },
    /// Remove item `index` from the sequence at `seq_path`, shifting every later
    /// item down one. `index` must be `< len`.
    RemoveItem { seq_path: Vec<Seg>, index: usize },
    /// Insert `key = value` into the mapping at `map_path` (the root when empty),
    /// **appended** after its existing entries.
    ///
    /// **Unspecified** when `key` is already present — an upserting backend
    /// overwrites in place, a strict one errors.
    InsertKey {
        map_path: Vec<Seg>,
        key: String,
        value: Value,
    },
    /// Append `value` to the sequence at `seq_path`, at index `len`.
    AppendItem { seq_path: Vec<Seg>, value: Value },
    /// Move the item at `from` to `to` in the sequence at `seq_path`: a removal
    /// followed by a reinsertion, so `to` is read against the sequence *with the
    /// item already taken out*, and the items between the two shift by one. The
    /// length is unchanged and both indices must be `< len`.
    ///
    /// An editor with no native move lowers this through
    /// [`move_permutation`] rather than deriving the index arithmetic again.
    MoveItem {
        seq_path: Vec<Seg>,
        from: usize,
        to: usize,
    },
    /// Reorder the mapping at `map_path` so its entries follow `keys`. `keys` is a
    /// permutation of the mapping's current keys: reordering moves entries, it
    /// never adds, drops, or renames one.
    ReorderKeys {
        map_path: Vec<Seg>,
        keys: Vec<String>,
    },
    /// Rename the mapping entry at `path` to `new_key`, keeping its value and its
    /// position in the key order. `path` must end in a [`Seg::Key`]; `new_key` must
    /// not collide with an existing sibling.
    RenameKey { path: Vec<Seg>, new_key: String },
}

/// Lower a [`EditOp::MoveItem`] into the index permutation that a
/// `reorder_items`-style primitive takes, for a backend whose editor has no native
/// move. `None` when either index is out of range for `len` — nothing to do.
///
/// The arithmetic is one line and wrong in two ways if you rederive it (whether
/// `to` counts the moved item, and which direction the middle shifts), and it is
/// generic to *any* backend over such an editor — so it lives here rather than in
/// each one.
pub fn move_permutation(len: usize, from: usize, to: usize) -> Option<Vec<usize>> {
    if from >= len || to >= len {
        return None;
    }
    let mut order: Vec<usize> = (0..len).collect();
    let moved = order.remove(from);
    order.insert(to, moved);
    Some(order)
}

/// A backend failure, carrying the underlying message. An error means the edit
/// did not apply; the document is unchanged.
#[derive(Debug)]
pub struct BackendError(pub String);

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BackendError {}

fn err(e: impl std::fmt::Display) -> BackendError {
    BackendError(e.to_string())
}

/// The editing surface `Model` drives. Implementors own the document's bytes and
/// apply edits losslessly.
pub trait Backend {
    /// Apply one edit. Atomic: on `Err`, the document is unchanged.
    fn apply(&mut self, op: EditOp) -> Result<(), BackendError>;

    /// The current value tree to render (for an embed backend, the metadata
    /// region — *not* the whole host file).
    fn to_value(&self) -> Result<Value, BackendError>;

    /// The canonical serialized form the embedder persists on save (for an embed
    /// backend, the full rendered host file).
    fn source(&self) -> Result<String, BackendError>;

    /// The schema governing this document, if the backend knows one. The backend
    /// is exactly the component that knows *where the document came from*, so it is
    /// the right place to know what governs it — a prov backend returns the schema
    /// resolved from the workspace config; a standalone config file has none.
    /// Defaulted to `None` so existing backends are unaffected.
    fn schema(&self) -> Option<crate::schema::Schema> {
        None
    }
}

/// A [`Backend`] over a standalone config file, backed by [`fig::Editor`].
pub struct FigBackend {
    editor: fig::Editor,
    format: Format,
}

impl FigBackend {
    /// Open an editor over a copy of `source` parsed as `format`.
    pub fn open(source: &[u8], format: Format) -> Result<Self, BackendError> {
        let editor = fig::Editor::open(source, format).map_err(err)?;
        Ok(Self { editor, format })
    }
}

impl Backend for FigBackend {
    fn apply(&mut self, op: EditOp) -> Result<(), BackendError> {
        match op {
            EditOp::ReplaceValue { path, value } => self
                .editor
                .replace_value(&tree::to_fig(&path), value)
                .map_err(err),
            EditOp::DeleteKey { path } => self.editor.delete(&tree::to_fig(&path)).map_err(err),
            EditOp::RemoveItem { seq_path, index } => self
                .editor
                .remove_item(&tree::to_fig(&seq_path), index)
                .map_err(err),
            EditOp::InsertKey {
                map_path,
                key,
                value,
            } => self
                .editor
                .insert_value(&tree::to_fig(&map_path), &key, value)
                .map_err(err),
            EditOp::AppendItem { seq_path, value } => self
                .editor
                .append_value(&tree::to_fig(&seq_path), value)
                .map_err(err),
            EditOp::MoveItem { seq_path, from, to } => self
                .editor
                .move_item(&tree::to_fig(&seq_path), from, to)
                .map_err(err),
            EditOp::ReorderKeys { map_path, keys } => self
                .editor
                .reorder_keys(&tree::to_fig(&map_path), &keys)
                .map_err(err),
            EditOp::RenameKey { path, new_key } => self
                .editor
                .replace_key(&tree::to_fig(&path), &new_key)
                .map_err(err),
        }
    }

    fn to_value(&self) -> Result<Value, BackendError> {
        let src = self.editor.source().map_err(err)?;
        let doc = fig::Document::parse(src.as_bytes(), self.format).map_err(err)?;
        doc.to_value().map_err(err)
    }

    fn source(&self) -> Result<String, BackendError> {
        self.editor.source().map(|s| s.to_string()).map_err(err)
    }
}

/// A contract suite any [`Backend`] implementation can run against [`EditOp`]'s
/// documented guarantees.
///
/// A trait with one implementation has no contract, only a behavior; the second
/// implementation is where the two silently part ways. This is the check that
/// catches that — a prov backend, an embed backend, or anything else built later
/// runs [`check`] in its own test module and finds out where it drifted.
///
/// It asserts only what [`EditOp`] actually promises: the two cases documented as
/// unspecified there are not probed, so a backend is free to differ on them.
pub mod conformance {
    use super::{Backend, EditOp};
    use crate::tree::{self, Seg};
    use fig::Value;

    /// The document shape every check starts from. A caller's `open` closure must
    /// hand back a fresh backend over a document equivalent to:
    ///
    /// ```text
    /// title = "note"
    /// tags  = ["alpha", "beta", "gamma"]
    /// nested = { k = "v", j = "w" }
    /// ```
    ///
    /// — written in whatever format that backend reads. [`FIXTURE_TOML`] is that
    /// document for a TOML-parsing backend; [`fixture`] is the tree it must parse
    /// to, which [`check`] verifies first so a mistyped fixture reports as itself
    /// rather than as nine failing ops.
    pub const FIXTURE_TOML: &str = "\
title = \"note\"
tags = [\"alpha\", \"beta\", \"gamma\"]

[nested]
k = \"v\"
j = \"w\"
";

    /// The value tree [`FIXTURE_TOML`] (or its equivalent in another format) parses
    /// to — the starting state each check assumes.
    pub fn fixture() -> Value {
        fn s(v: &str) -> Value {
            Value::Str(v.to_string())
        }
        Value::Map(vec![
            (s("title"), s("note")),
            (
                s("tags"),
                Value::Seq(vec![s("alpha"), s("beta"), s("gamma")]),
            ),
            (
                s("nested"),
                Value::Map(vec![(s("k"), s("v")), (s("j"), s("w"))]),
            ),
        ])
    }

    /// Everything that didn't hold, one entry per violated guarantee.
    ///
    /// `Debug` prints the same as `Display`, so `check(..).unwrap()` in a test
    /// reports readably instead of as one escaped line.
    pub struct Report(pub Vec<String>);

    impl std::fmt::Display for Report {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            writeln!(f, "{} backend contract violation(s):", self.0.len())?;
            for failure in &self.0 {
                writeln!(f, "  - {failure}")?;
            }
            Ok(())
        }
    }

    impl std::fmt::Debug for Report {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "\n{self}")
        }
    }

    fn key(k: &str) -> Seg {
        Seg::Key(k.to_string())
    }

    /// Run the suite. `open` must return a **fresh** backend over the [`fixture`]
    /// document on every call — each check re-opens, so one failure can't cascade
    /// into the next.
    ///
    /// ```no_run
    /// # use flower_core::backend::{FigBackend, conformance};
    /// # use fig::Format;
    /// conformance::check(|| {
    ///     FigBackend::open(conformance::FIXTURE_TOML.as_bytes(), Format::Toml).unwrap()
    /// })
    /// .unwrap();
    /// ```
    pub fn check<B: Backend>(open: impl Fn() -> B) -> Result<(), Report> {
        let mut failures = Vec::new();

        // Precondition: the caller's fixture really is the document the rest of
        // the suite reasons about. Bail rather than report the fallout.
        match open().to_value() {
            Ok(v) if v == fixture() => {}
            Ok(v) => {
                return Err(Report(vec![format!(
                    "`open` does not yield the fixture document.\n      expected: {:?}\n      got:      {v:?}",
                    fixture()
                )]));
            }
            Err(e) => return Err(Report(vec![format!("`open().to_value()` failed: {e}")])),
        }

        // Apply `op` to a fresh document and hand the resulting tree to `assert`,
        // which names whatever went wrong.
        let mut case = |name: &str, op: EditOp, assert: &dyn Fn(&Value) -> Option<String>| {
            let mut backend = open();
            match backend.apply(op) {
                Err(e) => failures.push(format!("{name}: apply failed: {e}")),
                Ok(()) => match backend.to_value() {
                    Err(e) => failures.push(format!("{name}: to_value failed: {e}")),
                    Ok(tree) => {
                        if let Some(why) = assert(&tree) {
                            failures.push(format!("{name}: {why}"));
                        }
                    }
                },
            }
        };

        // Compare the node at `path` against `want`, and name the mismatch.
        let at = |tree: &Value, path: &[Seg], want: Value| -> Option<String> {
            match tree::value_at(tree, path) {
                Some(got) if *got == want => None,
                Some(got) => Some(format!("at {path:?}: expected {want:?}, got {got:?}")),
                None => Some(format!("at {path:?}: path did not resolve")),
            }
        };
        let str_v = |v: &str| Value::Str(v.to_string());
        let strs = |vs: &[&str]| Value::Seq(vs.iter().map(|v| Value::Str(v.to_string())).collect());

        // ── ReplaceValue: in place, siblings untouched ────────────────────────
        case(
            "ReplaceValue on a scalar key",
            EditOp::ReplaceValue {
                path: vec![key("title")],
                value: str_v("REPLACED"),
            },
            &|t| {
                at(t, &[key("title")], str_v("REPLACED"))
                    .or_else(|| at(t, &[key("tags")], strs(&["alpha", "beta", "gamma"])))
            },
        );
        case(
            "ReplaceValue on a sequence item",
            EditOp::ReplaceValue {
                path: vec![key("tags"), Seg::Index(1)],
                value: str_v("REPLACED"),
            },
            &|t| at(t, &[key("tags")], strs(&["alpha", "REPLACED", "gamma"])),
        );
        // A subtree is a value like any other: replacing a mapping with a scalar
        // must not merge into what was there.
        case(
            "ReplaceValue on a container",
            EditOp::ReplaceValue {
                path: vec![key("nested")],
                value: str_v("REPLACED"),
            },
            &|t| at(t, &[key("nested")], str_v("REPLACED")),
        );

        // ── DeleteKey / RemoveItem ────────────────────────────────────────────
        case(
            "DeleteKey",
            EditOp::DeleteKey {
                path: vec![key("nested"), key("k")],
            },
            &|t| {
                at(
                    t,
                    &[key("nested")],
                    Value::Map(vec![(str_v("j"), str_v("w"))]),
                )
                .or_else(|| at(t, &[key("title")], str_v("note")))
            },
        );
        case(
            "RemoveItem shifts later items down",
            EditOp::RemoveItem {
                seq_path: vec![key("tags")],
                index: 0,
            },
            &|t| at(t, &[key("tags")], strs(&["beta", "gamma"])),
        );

        // ── InsertKey / AppendItem: appended, existing entries kept ───────────
        case(
            "InsertKey into a nested mapping",
            EditOp::InsertKey {
                map_path: vec![key("nested")],
                key: "added".to_string(),
                value: str_v("x"),
            },
            &|t| {
                at(
                    t,
                    &[key("nested")],
                    Value::Map(vec![
                        (str_v("k"), str_v("v")),
                        (str_v("j"), str_v("w")),
                        (str_v("added"), str_v("x")),
                    ]),
                )
            },
        );
        case(
            "InsertKey at the root (empty path)",
            EditOp::InsertKey {
                map_path: Vec::new(),
                key: "added".to_string(),
                value: str_v("x"),
            },
            &|t| {
                at(t, &[key("added")], str_v("x")).or_else(|| at(t, &[key("title")], str_v("note")))
            },
        );
        case(
            "AppendItem lands at the end",
            EditOp::AppendItem {
                seq_path: vec![key("tags")],
                value: str_v("delta"),
            },
            &|t| {
                at(
                    t,
                    &[key("tags")],
                    strs(&["alpha", "beta", "gamma", "delta"]),
                )
            },
        );

        // ── MoveItem: remove-then-reinsert, both directions ───────────────────
        case(
            "MoveItem backwards",
            EditOp::MoveItem {
                seq_path: vec![key("tags")],
                from: 2,
                to: 0,
            },
            &|t| at(t, &[key("tags")], strs(&["gamma", "alpha", "beta"])),
        );
        case(
            "MoveItem forwards",
            EditOp::MoveItem {
                seq_path: vec![key("tags")],
                from: 0,
                to: 2,
            },
            &|t| at(t, &[key("tags")], strs(&["beta", "gamma", "alpha"])),
        );

        // ── ReorderKeys / RenameKey ───────────────────────────────────────────
        // Deliberately on the *nested* mapping: a root-level reorder is the same op
        // but some formats constrain what may follow a section header, and that is
        // the format's business rather than the backend's.
        case(
            "ReorderKeys",
            EditOp::ReorderKeys {
                map_path: vec![key("nested")],
                keys: vec!["j".to_string(), "k".to_string()],
            },
            &|t| {
                at(
                    t,
                    &[key("nested")],
                    Value::Map(vec![(str_v("j"), str_v("w")), (str_v("k"), str_v("v"))]),
                )
            },
        );
        case(
            "RenameKey keeps the value and the position",
            EditOp::RenameKey {
                path: vec![key("nested"), key("k")],
                new_key: "renamed".to_string(),
            },
            &|t| {
                at(
                    t,
                    &[key("nested")],
                    Value::Map(vec![
                        (str_v("renamed"), str_v("v")),
                        (str_v("j"), str_v("w")),
                    ]),
                )
            },
        );

        // ── Atomicity: a rejected op leaves the document exactly as it was ────
        // Whether an out-of-range index errors or is declined as a no-op is the
        // backend's call; that the document survives it is not.
        for (name, op) in [
            (
                "RemoveItem past the end",
                EditOp::RemoveItem {
                    seq_path: vec![key("tags")],
                    index: 99,
                },
            ),
            (
                "MoveItem past the end",
                EditOp::MoveItem {
                    seq_path: vec![key("tags")],
                    from: 0,
                    to: 99,
                },
            ),
            (
                "DeleteKey on an absent key",
                EditOp::DeleteKey {
                    path: vec![key("nope")],
                },
            ),
            (
                "AppendItem onto a non-sequence",
                EditOp::AppendItem {
                    seq_path: vec![key("title")],
                    value: str_v("x"),
                },
            ),
        ] {
            let mut backend = open();
            let _ = backend.apply(op);
            match backend.to_value() {
                Ok(tree) if tree == fixture() => {}
                Ok(tree) => failures.push(format!(
                    "{name}: document changed by a rejected op.\n      expected: {:?}\n      got:      {tree:?}",
                    fixture()
                )),
                Err(e) => {
                    failures.push(format!("{name}: document unreadable after a rejected op: {e}"))
                }
            }
        }

        // ── source(): the edit reached the bytes, not just the tree ───────────
        {
            let mut backend = open();
            let op = EditOp::ReplaceValue {
                path: vec![key("title")],
                value: str_v("REPLACED"),
            };
            match backend.apply(op).and_then(|()| backend.source()) {
                Ok(src) if src.contains("REPLACED") => {
                    if !src.contains("gamma") {
                        failures.push(
                            "source() after an edit dropped an untouched sibling value".to_string(),
                        );
                    }
                }
                Ok(src) => failures.push(format!(
                    "source() does not carry the committed edit:\n{src}"
                )),
                Err(e) => failures.push(format!("source() after an edit failed: {e}")),
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(Report(failures))
        }
    }

    /// `move_permutation` is the lowering backends share, so its own arithmetic is
    /// checked here rather than in each of them.
    #[cfg(test)]
    mod permutation_tests {
        use crate::backend::move_permutation;

        #[test]
        fn move_permutation_matches_remove_then_reinsert() {
            assert_eq!(move_permutation(3, 2, 0), Some(vec![2, 0, 1]));
            assert_eq!(move_permutation(3, 0, 2), Some(vec![1, 2, 0]));
            assert_eq!(move_permutation(3, 1, 1), Some(vec![0, 1, 2]));
            assert_eq!(move_permutation(3, 0, 3), None);
            assert_eq!(move_permutation(0, 0, 0), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_fixture() -> FigBackend {
        FigBackend::open(conformance::FIXTURE_TOML.as_bytes(), Format::Toml).expect("open fixture")
    }

    /// flower's own backend is the suite's first subject.
    ///
    /// It passes every guarantee but one, and that one is an upstream defect
    /// rather than a contract that was written too strictly: `ReplaceValue` at a
    /// TOML **table-header** table (`[nested]`) rewrites the *header's key* instead
    /// of the table's value, so replacing `nested` with `"REPLACED"` silently
    /// renames the section to `["REPLACED"]` and reports `Ok`. It is specific to
    /// that one shape — an inline table, an array, and every YAML/JSON container
    /// replace correctly.
    ///
    /// The deviation is pinned here rather than dropped from
    /// [`conformance::check`], so it cannot grow quietly and so this test turns red
    /// (and the block goes away) the day fig fixes it.
    #[test]
    fn fig_backend_satisfies_the_edit_op_contract_but_for_a_known_fig_defect() {
        let report = conformance::check(open_fixture)
            .expect_err("if this now passes, delete the allowance below");
        assert_eq!(
            report.0.len(),
            1,
            "only the known deviation is allowed:{report}"
        );
        assert!(
            report.0[0].starts_with("ReplaceValue on a container"),
            "unexpected deviation:{report}"
        );
    }

    /// The corruption itself, stated as behavior rather than as a count — so the
    /// hazard is legible to anyone reaching for `Model::set_value_at` on a TOML
    /// table, and so a *change* in how fig gets it wrong is caught too.
    #[test]
    fn replacing_a_toml_table_header_renames_the_section_upstream() {
        let mut backend = open_fixture();
        backend
            .apply(EditOp::ReplaceValue {
                path: vec![Seg::Key("nested".into())],
                value: Value::Str("REPLACED".into()),
            })
            .expect("fig reports success");
        let src = backend.source().expect("source");
        assert!(src.contains("[\"REPLACED\"]"), "header renamed:\n{src}");
        assert!(src.contains("k = \"v\""), "table body left behind:\n{src}");
    }
}
