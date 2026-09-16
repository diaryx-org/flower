//! flower-core's own constraint vocabulary, plugged into fig-schema's generic
//! rule engine.
//!
//! fig-schema's [`fig_schema::FieldRule`]/[`fig_schema::Schema`] are generic
//! over the constraint type; this module supplies flower's: a controlled
//! vocabulary ([`Constraint::Enum`]) or a link field
//! ([`Constraint::Reference`], where *spanning* lives — the discovery
//! backbone). flower-core never learns the word "prov": an embedder (a prov
//! adapter, a `$schema` detector, …) builds a [`Schema`] and supplies it —
//! either through [`Backend::schema`](crate::Backend::schema) or by injecting
//! it into the [`Model`](crate::Model), the same way managed keys arrive today.

use fig::Value;
use fig_schema::{Cardinality, Term, Validate, Validation, validate_enum};

/// flower's field rule and schema, with flower's own [`Constraint`] plugged
/// into fig-schema's generic engine.
pub type FieldRule = fig_schema::FieldRule<Constraint>;
pub type Schema = fig_schema::Schema<Constraint>;

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

impl Validate for Constraint {
    /// Only a controlled vocabulary constrains scalar *values* here; a
    /// reference or a type-only rule imposes nothing (fig's reparse remains
    /// the backstop).
    fn validate(&self, value: &Value) -> Validation {
        match self {
            Constraint::Enum { values, closed } => validate_enum(values, *closed, value),
            Constraint::Reference { .. } => Validation::Ok,
        }
    }
}

/// One offered value for a field, and how it reads.
///
/// What a picker is a list of. Two very different things produce them and a
/// frontend renders both the same way: a controlled vocabulary's terms, which
/// the schema carries, and a reference field's candidates, which it cannot —
/// a link points at *other documents*, and flower-core is one document with no
/// filesystem, so the host answers ([`Backend::candidates`](crate::Backend::candidates)).
/// That is the injection point `design/schema.md` filed as the open question.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    /// What committing this writes into the document — a `Value`, not a string,
    /// because a vocabulary of numbers or booleans is as legal as one of names.
    pub value: Value,
    /// What the list shows, and what a filter matches against.
    pub label: String,
    /// A second line: a term's gloss, `retired` for one no longer offered, the
    /// title of the document a link points at. `None` when there is nothing to
    /// add.
    pub detail: Option<String>,
}

impl Choice {
    /// A choice whose label is its own value text.
    pub fn new(value: Value, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            detail: None,
        }
    }

    /// A string choice that shows and stores the same text.
    pub fn plain(value: impl Into<String>) -> Self {
        let value = value.into();
        Self::new(Value::Str(value.clone()), value)
    }

    /// Set the second line.
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Whether this choice matches a picker's filter — a case-insensitive
    /// substring of the label.
    ///
    /// On the label rather than on the stored value, because the label is what
    /// the reader can see: filtering a list of link targets by their ids would
    /// be filtering on the one part of the row nobody is reading.
    pub fn matches(&self, filter: &str) -> bool {
        filter.is_empty() || self.label.to_lowercase().contains(&filter.to_lowercase())
    }
}

/// The choices a controlled vocabulary offers, in the order it declares them.
///
/// A retired term is *offered*, and flagged: it is still legal where it is
/// already written (validating one warns rather than rejects), so leaving it
/// out of the list would make a document holding one unre-choosable from the
/// picker — the reader would have to retype what is already there.
pub fn choices_of(terms: &[Term]) -> Vec<Choice> {
    terms
        .iter()
        .map(|t| {
            let detail = match (t.retired, t.description.as_deref()) {
                (true, Some(d)) => Some(format!("retired — {d}")),
                (true, None) => Some("retired".to_string()),
                (false, d) => d.map(str::to_string),
            };
            Choice {
                value: Value::Str(t.value.clone()),
                label: t.display_label().to_string(),
                detail,
            }
        })
        .collect()
}

/// Convenience accessors for a flower [`FieldRule`], mirroring what used to be
/// inherent methods before [`fig_schema::FieldRule`] became generic. A local
/// trait, since inherent impls can't be added to a foreign generic type.
pub trait FieldRuleExt {
    /// The controlled vocabulary this rule enforces, if any.
    fn enum_constraint(&self) -> Option<(&[Term], bool)>;
    /// The reference/relation this rule describes, if any.
    fn reference(&self) -> Option<&str>;
}

impl FieldRuleExt for FieldRule {
    fn enum_constraint(&self) -> Option<(&[Term], bool)> {
        match &self.constraint {
            Some(Constraint::Enum { values, closed }) => Some((values, *closed)),
            _ => None,
        }
    }

    fn reference(&self) -> Option<&str> {
        match &self.constraint {
            Some(Constraint::Reference { relation, .. }) => Some(relation),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Seg;
    use fig_schema::{FieldType, Issue, PathPat};

    #[test]
    fn closed_enum_rejects_unknown_accepts_known() {
        let rule = FieldRule::new(PathPat::each_item_of("audience"))
            .ty(FieldType::Str)
            .constraint(Constraint::Enum {
                values: vec![Term::value("public"), Term::value("private")],
                closed: true,
            });
        assert_eq!(rule.validate(&Value::Str("public".into())), Validation::Ok);
        assert!(matches!(
            rule.validate(&Value::Str("familly".into())),
            Validation::Reject(_)
        ));
    }

    #[test]
    fn reference_constraint_is_not_checked_here() {
        let rule = FieldRule::new(PathPat::key("part_of"))
            .ty(FieldType::Ref)
            .constraint(Constraint::Reference {
                relation: "part_of".into(),
                cardinality: Cardinality::One,
                spanning: true,
            });
        assert_eq!(
            rule.validate(&Value::Str("anything".into())),
            Validation::Ok
        );
        assert_eq!(rule.reference(), Some("part_of"));
    }

    #[test]
    fn schema_rule_for_matches_by_path() {
        let schema = Schema::new(vec![FieldRule::new(PathPat::key("status")).constraint(
            Constraint::Enum {
                values: vec![Term::value("active"), Term::value("archived").retired(true)],
                closed: true,
            },
        )]);
        let rule = schema.rule_for(&[Seg::Key("status".into())]).unwrap();
        assert_eq!(rule.validate(&Value::Str("active".into())), Validation::Ok);
        // A retired term is still a *member*, so even a closed vocabulary only
        // warns — not the same failure as a value nobody ever declared.
        assert_eq!(
            rule.validate(&Value::Str("archived".into())),
            Validation::Warn(Issue::retired("archived"))
        );
        // A value outside the vocabulary is the hard rejection, and carries the
        // near-miss a frontend can offer as a one-tap correction.
        let unknown = rule.validate(&Value::Str("activ".into()));
        assert!(unknown.is_reject());
        assert_eq!(
            unknown.issue().and_then(|i| i.suggestion.as_deref()),
            Some("active")
        );
    }
}
