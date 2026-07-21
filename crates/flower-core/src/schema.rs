//! The schema layer: what a field *expects* — its type, its allowed values, and
//! how to present it — layered over fig's schema-free value tree.
//!
//! fig parses bytes → [`Value`] and edits losslessly; it has no notion of "what
//! is valid here". This module adds that knowledge as a **generic, prov-agnostic**
//! type. flower-core never learns the word "prov": an embedder (a prov adapter, a
//! `$schema` detector, …) builds a [`Schema`] and supplies it — either through
//! [`Backend::schema`](crate::Backend::schema) or by injecting it into the
//! [`Model`](crate::Model), the same way managed keys arrive today.
//!
//! Two things the schema knows, both used at the model's existing choke points:
//!
//! - **the expected [`FieldType`]** — so an edit buffer is coerced to the type the
//!   field wants (a `str` field keeps `"123"` a string) instead of guessed by
//!   literal shape ([`crate::tree::parse_scalar`]);
//! - **a [`Constraint`]** — a controlled vocabulary ([`Constraint::Enum`]) whose
//!   closed form rejects unknown values at commit, or a link field
//!   ([`Constraint::Reference`], where *spanning* lives).
//!
//! Presentation ([`Presentation`]) is renderer-neutral: semantic [`Icon`]/[`Tint`]
//! hints a SwiftUI or ratatui frontend maps to its own symbols and colours.

use fig::Value;

use crate::tree::Seg;

/// A set of field rules. Matched against a row's fig path to find what governs it.
#[derive(Debug, Clone, Default)]
pub struct Schema {
    rules: Vec<FieldRule>,
}

impl Schema {
    /// Build a schema from its rules.
    pub fn new(rules: Vec<FieldRule>) -> Self {
        Self { rules }
    }

    /// The rules, in declaration order.
    pub fn rules(&self) -> &[FieldRule] {
        &self.rules
    }

    /// Whether the schema carries no rules (nothing to apply).
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The first rule whose pattern matches `path`, if any. Declaration order is
    /// precedence, so a more specific rule should be listed before a broader one.
    pub fn rule_for(&self, path: &[Seg]) -> Option<&FieldRule> {
        self.rules.iter().find(|r| r.at.matches(path))
    }
}

/// One field rule: which node(s) it governs, the type it expects, an optional
/// constraint, and how to present it.
#[derive(Debug, Clone)]
pub struct FieldRule {
    /// Which node(s) this governs (reaches list *elements*, not only scalars).
    pub at: PathPat,
    /// The expected type — drives type-directed parsing and widget choice.
    pub ty: Option<FieldType>,
    /// A value constraint (controlled vocabulary or reference).
    pub constraint: Option<Constraint>,
    /// Renderer-neutral presentation hints.
    pub present: Presentation,
}

impl FieldRule {
    /// The controlled vocabulary this rule enforces, if any.
    pub fn enum_constraint(&self) -> Option<(&[Term], bool)> {
        match &self.constraint {
            Some(Constraint::Enum { values, closed }) => Some((values, *closed)),
            _ => None,
        }
    }

    /// The reference/relation this rule describes, if any.
    pub fn reference(&self) -> Option<&str> {
        match &self.constraint {
            Some(Constraint::Reference { relation, .. }) => Some(relation),
            _ => None,
        }
    }
}

/// A path pattern. Unlike a concrete fig path it can reach every element of a
/// sequence ([`SegPat::EachItem`]) or every entry of a map ([`SegPat::AnyKey`]),
/// so a rule can constrain *each item* of a list field (`tags:`, `audience:`).
#[derive(Debug, Clone)]
pub struct PathPat(pub Vec<SegPat>);

/// One step of a [`PathPat`].
#[derive(Debug, Clone)]
pub enum SegPat {
    /// An exact mapping key.
    Key(String),
    /// Any mapping key at this depth.
    AnyKey,
    /// An exact sequence index.
    Index(usize),
    /// Any sequence item at this depth.
    EachItem,
}

impl PathPat {
    /// A single top-level key — the common case (`audience`, `title`).
    pub fn key(name: impl Into<String>) -> Self {
        PathPat(vec![SegPat::Key(name.into())])
    }

    /// A top-level list field whose *each item* the rule governs
    /// (`audience:` as a sequence).
    pub fn each_item_of(name: impl Into<String>) -> Self {
        PathPat(vec![SegPat::Key(name.into()), SegPat::EachItem])
    }

    /// Whether this pattern matches the concrete fig `path` (same length, each
    /// segment compatible).
    pub fn matches(&self, path: &[Seg]) -> bool {
        if self.0.len() != path.len() {
            return false;
        }
        self.0.iter().zip(path).all(|(pat, seg)| match (pat, seg) {
            (SegPat::Key(k), Seg::Key(s)) => k == s,
            (SegPat::AnyKey, Seg::Key(_)) => true,
            (SegPat::Index(i), Seg::Index(j)) => i == j,
            (SegPat::EachItem, Seg::Index(_)) => true,
            _ => false,
        })
    }
}

/// The type a field expects. Drives type-directed parsing and widget choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Null,
    Bool,
    Int,
    Float,
    Str,
    /// A link into the workspace (stored textually, like `Str`, but a reference).
    Ref,
    Map,
    Seq,
}

impl FieldType {
    /// Coerce an edit-buffer string to this type — the schema-directed counterpart
    /// of [`crate::tree::parse_scalar`]. A value that doesn't fit the type falls
    /// back to a string (fig's reparse is the final backstop); container types are
    /// not scalar-edited, so they also pass through as text.
    pub fn coerce(self, s: &str) -> Value {
        let t = s.trim();
        match self {
            FieldType::Null => Value::Null,
            FieldType::Bool => match t {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => Value::Str(s.to_string()),
            },
            FieldType::Int => t
                .parse::<i64>()
                .map(Value::Int)
                .or_else(|_| t.parse::<u64>().map(Value::Uint))
                .unwrap_or_else(|_| Value::Str(s.to_string())),
            FieldType::Float => t
                .parse::<f64>()
                .map(Value::Float)
                .unwrap_or_else(|_| Value::Str(s.to_string())),
            // A string/ref field keeps its literal text — the whole point of
            // type-directed parsing: `"123"` in a `str` field stays a string.
            FieldType::Str | FieldType::Ref | FieldType::Map | FieldType::Seq => {
                Value::Str(s.to_string())
            }
        }
    }
}

/// A value constraint on a field.
#[derive(Debug, Clone)]
pub enum Constraint {
    /// A controlled vocabulary — an enumerated set of allowed values.
    Enum {
        /// The legal terms.
        values: Vec<Term>,
        /// `true`: an unknown value is rejected. `false`: allowed, near-misses warn.
        closed: bool,
    },
    /// A relation / link field. The spanning (containment) backbone lives here.
    Reference {
        /// The relation name (`contents`, `part_of`, …).
        relation: String,
        /// Single link vs a list of links.
        cardinality: Cardinality,
        /// The spanning containment relation (the discovery backbone).
        spanning: bool,
    },
}

/// Whether a reference holds one link or many.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    One,
    Many,
}

/// One term of a controlled vocabulary.
#[derive(Debug, Clone)]
pub struct Term {
    /// The stored value.
    pub value: String,
    /// A human display label (defaults to `value`).
    pub label: Option<String>,
    /// A human gloss / help text.
    pub description: Option<String>,
    /// Known but no longer offered: still renders if already present, excluded
    /// from the picker's offered set.
    pub retired: bool,
    /// A per-value tint (e.g. `public` = positive/green).
    pub tint: Option<Tint>,
}

impl Term {
    /// A bare live term with just a value.
    pub fn value(v: impl Into<String>) -> Self {
        Self {
            value: v.into(),
            label: None,
            description: None,
            retired: false,
            tint: None,
        }
    }
}

/// Renderer-neutral presentation hints. A frontend maps these to its own symbols
/// and colours (SwiftUI → SF Symbols + adaptive `Color`; ratatui → unicode + ANSI).
#[derive(Debug, Clone, Default)]
pub struct Presentation {
    /// A human field label.
    pub title: Option<String>,
    /// Help text / section subtitle.
    pub description: Option<String>,
    /// A semantic icon.
    pub icon: Option<Icon>,
    /// A semantic tint.
    pub tint: Option<Tint>,
}

/// A semantic icon hint. Frontends map to their own symbol set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Icon {
    Link,
    Enum,
    Toggle,
    Lock,
    Globe,
    Clock,
    Tag,
    Text,
    /// An escape hatch naming a frontend-specific symbol.
    Other(String),
}

/// A semantic tint hint. Frontends map to theme-adaptive colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    Accent,
    Neutral,
    Positive,
    Warning,
    Danger,
}

/// The result of validating a value against a field's constraint at commit time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validation {
    /// The value is fine — apply it.
    Ok,
    /// A soft warning (an open vocabulary's unknown value); apply, but surface it.
    Warn(String),
    /// A hard rejection (a closed vocabulary's unknown value); do not apply.
    Reject(String),
}

impl FieldRule {
    /// Validate a candidate `value` against this rule's constraint. Only a
    /// controlled vocabulary constrains scalar *values*; a reference or a
    /// type-only rule imposes nothing here (fig's reparse remains the backstop).
    pub fn validate(&self, value: &Value) -> Validation {
        let Some((terms, closed)) = self.enum_constraint() else {
            return Validation::Ok;
        };
        // Only string-valued terms are checked; a non-string value under an enum
        // field is left to the backend's reparse.
        let Value::Str(s) = value else {
            return Validation::Ok;
        };
        if terms.iter().any(|t| !t.retired && &t.value == s) {
            return Validation::Ok;
        }
        if closed {
            Validation::Reject(format!("“{s}” is not an allowed value"))
        } else if let Some(near) = nearest_term(terms, s) {
            Validation::Warn(format!("“{s}” is unknown — did you mean “{near}”?"))
        } else {
            Validation::Warn(format!("“{s}” is not a known value"))
        }
    }
}

/// The live term closest to `value` by case-insensitive equality or a small
/// edit distance — a lightweight near-miss suggestion for an open vocabulary.
fn nearest_term(terms: &[Term], value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    terms
        .iter()
        .filter(|t| !t.retired)
        .map(|t| (t.value.clone(), edit_distance(&t.value.to_ascii_lowercase(), &lower)))
        // Only suggest a genuinely close term (one or two typos on a short word).
        .filter(|(_, d)| *d <= 2)
        .min_by_key(|(_, d)| *d)
        .map(|(v, _)| v)
}

/// Levenshtein distance — a tiny dependency-free implementation for near-miss
/// suggestions (the vocabulary sets here are small).
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_pattern_matches_keys_and_each_item() {
        let pat = PathPat::each_item_of("audience");
        assert!(pat.matches(&[Seg::Key("audience".into()), Seg::Index(0)]));
        assert!(pat.matches(&[Seg::Key("audience".into()), Seg::Index(3)]));
        assert!(!pat.matches(&[Seg::Key("audience".into())]));
        assert!(!pat.matches(&[Seg::Key("tags".into()), Seg::Index(0)]));
    }

    #[test]
    fn type_directed_parse_keeps_a_string_field_a_string() {
        assert_eq!(FieldType::Str.coerce("123"), Value::Str("123".into()));
        assert_eq!(FieldType::Int.coerce("123"), Value::Int(123));
        assert_eq!(FieldType::Bool.coerce("true"), Value::Bool(true));
        // A non-fitting value falls back to a string (reparse is the backstop).
        assert_eq!(FieldType::Int.coerce("abc"), Value::Str("abc".into()));
    }

    #[test]
    fn closed_enum_rejects_unknown_accepts_known() {
        let rule = FieldRule {
            at: PathPat::each_item_of("audience"),
            ty: Some(FieldType::Str),
            constraint: Some(Constraint::Enum {
                values: vec![Term::value("public"), Term::value("private")],
                closed: true,
            }),
            present: Presentation::default(),
        };
        assert_eq!(rule.validate(&Value::Str("public".into())), Validation::Ok);
        assert!(matches!(
            rule.validate(&Value::Str("familly".into())),
            Validation::Reject(_)
        ));
    }

    #[test]
    fn open_enum_warns_with_a_near_miss() {
        let rule = FieldRule {
            at: PathPat::each_item_of("tags"),
            ty: Some(FieldType::Str),
            constraint: Some(Constraint::Enum {
                values: vec![Term::value("todo"), Term::value("done")],
                closed: false,
            }),
            present: Presentation::default(),
        };
        match rule.validate(&Value::Str("todi".into())) {
            Validation::Warn(msg) => assert!(msg.contains("todo"), "got: {msg}"),
            other => panic!("expected a near-miss warning, got {other:?}"),
        }
    }

    #[test]
    fn retired_term_is_not_accepted() {
        let rule = FieldRule {
            at: PathPat::key("status"),
            ty: None,
            constraint: Some(Constraint::Enum {
                values: vec![
                    Term::value("active"),
                    Term {
                        retired: true,
                        ..Term::value("archived")
                    },
                ],
                closed: true,
            }),
            present: Presentation::default(),
        };
        assert_eq!(rule.validate(&Value::Str("active".into())), Validation::Ok);
        assert!(matches!(
            rule.validate(&Value::Str("archived".into())),
            Validation::Reject(_)
        ));
    }
}
