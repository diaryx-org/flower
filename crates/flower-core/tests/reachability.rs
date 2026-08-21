//! Every fig-schema type an embedder needs is reachable *through* `flower_core`.
//!
//! An integration test rather than a unit one, deliberately: it compiles
//! outside the crate and so sees exactly what a host sees — which is the only
//! vantage point from which the thing being tested is visible at all. From
//! inside, every fig-schema name is in scope whether it is re-exported or not.
//!
//! ## Why this exists
//!
//! flower's facade offers embedders a single dependency: a host takes
//! `flower-core` and never names fig-schema. That promise is kept by the
//! re-export list in `lib.rs`, and it is quietly broken every time a type is
//! added upstream and not added there.
//!
//! It broke exactly that way once. `FieldRule` and `Schema` are plain type
//! aliases, so a new *inherent method* on either — `consequences_of`,
//! `severity_of` — reaches a host with no edit to flower at all. A new *type*
//! does not. A host could therefore call `severity_of` and have nowhere to put
//! the `Severity` it returned: a compile error a long way from its cause, in a
//! repo whose manifest looked complete.
//!
//! No semver check catches it. Adding a type upstream is additive and passes;
//! it simply remains unreachable. This is the check that does.

use fig::Value;
use flower_core::schema::Constraint;
use flower_core::{
    Consequence, FieldRule, FieldType, PathPat, Presentation, Severity, Term, Tint,
    guards_without_terms,
};

/// A host declares a rule carrying consequences, reads them back, and never
/// names fig-schema — the whole facade promise in one function.
#[test]
fn an_embedder_declares_and_reads_consequences_through_flower_alone() {
    let rule: FieldRule = FieldRule::new(PathPat::key("recycle_bin"))
        .ty(FieldType::Bool)
        .present(Presentation::default().title("Recoverable delete"))
        .on_change(Consequence::always("Reopens the vault."))
        .on_change(
            Consequence::when(false, "Deleting stops being recoverable.")
                .severity(Severity::ConfirmExplicitly),
        );

    // Landing on `false` incurs both: the blanket cost and the value's own.
    // Both sentences survive — the worst one does not swallow the other.
    let both = rule.consequences_of(&Value::Bool(false));
    assert_eq!(both.len(), 2);
    assert_eq!(
        rule.severity_of(&Value::Bool(false)),
        Some(Severity::ConfirmExplicitly)
    );

    // Landing on `true` incurs only the blanket one.
    assert_eq!(rule.consequences_of(&Value::Bool(true)).len(), 1);
    assert_eq!(rule.severity_of(&Value::Bool(true)), Some(Severity::Notice));
}

/// The lint is a free function, not a method, so it is the one piece of the
/// vocabulary that a type alias could never have carried across on its own.
#[test]
fn the_guard_lint_is_reachable_too() {
    let consequences = vec![Consequence::when("of", "typo for `off`")];
    let terms = [Term::value("off"), Term::value("manual")];
    assert_eq!(guards_without_terms(&consequences, &terms), vec!["of"]);
}

/// `Tint::ALL` exists so a host can assert it handles every tint. Reaching it
/// through flower is the point; using it is the host's business.
#[test]
fn the_tint_vocabulary_can_be_enumerated_through_flower() {
    assert!(Tint::ALL.contains(&Tint::Danger));
}

/// A rule that declares nothing costs nothing — the ordinary case, and the one
/// that must not start returning `Some`.
#[test]
fn a_rule_with_no_declared_cost_reports_none() {
    let rule: FieldRule = FieldRule::new(PathPat::key("title")).ty(FieldType::Str);
    assert!(
        rule.consequences_of(&Value::Str("anything".into()))
            .is_empty()
    );
    assert_eq!(rule.severity_of(&Value::Str("anything".into())), None);
    let _ = Constraint::Enum {
        values: vec![],
        closed: false,
    };
}
