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
    use fig_schema::{FieldType, Issue, PathPat, Presentation};

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
    fn reference_constraint_is_not_checked_here() {
        let rule = FieldRule {
            at: PathPat::key("part_of"),
            ty: Some(FieldType::Ref),
            constraint: Some(Constraint::Reference {
                relation: "part_of".into(),
                cardinality: Cardinality::One,
                spanning: true,
            }),
            present: Presentation::default(),
        };
        assert_eq!(
            rule.validate(&Value::Str("anything".into())),
            Validation::Ok
        );
        assert_eq!(rule.reference(), Some("part_of"));
    }

    #[test]
    fn schema_rule_for_matches_by_path() {
        let schema = Schema::new(vec![FieldRule {
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
        }]);
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
