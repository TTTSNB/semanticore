//! Tests for the SHACL, Datalog, OWL and storage result/error types: model-
//! based properties for the types with behaviour, and a Display check for
//! every error variant.

mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error as _;

use common::*;
use proptest::prelude::*;
use semanticore_core::datalog::{
    Atom, BodyLiteral, DatalogError, DatalogProgram, DatalogResult, DatalogTerm, Rule,
};
use semanticore_core::model::{NamedNode, NamedOrBlankNode, Term};
use semanticore_core::owl::{
    Axiom, ClassHierarchy, ClassIri, ConsistencyResult, EquivalenceClass, IndividualIri,
    InstanceRealization, OwlError,
};
use semanticore_core::shacl::{
    NodeValidationResult, PropertyPath, Severity, ShaclError, ShaclValidationReport,
    ValidationResult,
};
use semanticore_core::storage::{
    StorageCapabilities, StorageCapability, StorageError, TransactionIsolation,
};

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn arb_severity() -> impl Strategy<Value = Severity> {
    prop_oneof![
        Just(Severity::Violation),
        Just(Severity::Warning),
        Just(Severity::Info)
    ]
}

fn arb_path() -> impl Strategy<Value = PropertyPath> {
    let leaf = small_named().prop_map(PropertyPath::Predicate);
    leaf.prop_recursive(3, 16, 3, |inner| {
        prop_oneof![
            inner
                .clone()
                .prop_map(|p| PropertyPath::Inverse(Box::new(p))),
            prop::collection::vec(inner.clone(), 0..3).prop_map(PropertyPath::Sequence),
            prop::collection::vec(inner.clone(), 0..3).prop_map(PropertyPath::Alternative),
            inner
                .clone()
                .prop_map(|p| PropertyPath::ZeroOrMore(Box::new(p))),
            inner
                .clone()
                .prop_map(|p| PropertyPath::OneOrMore(Box::new(p))),
            inner.prop_map(|p| PropertyPath::ZeroOrOne(Box::new(p))),
        ]
    })
}

fn arb_result() -> impl Strategy<Value = ValidationResult> {
    (
        small_term(),
        arb_severity(),
        small_named(),
        maybe(arb_path()),
        maybe(small_term()),
        maybe(small_subject()),
        maybe("[a-z ]{0,8}"),
    )
        .prop_map(
            |(focus, sev, comp, path, value, shape, msg)| ValidationResult {
                focus_node: focus,
                result_severity: sev,
                source_constraint_component: comp,
                result_path: path,
                value,
                source_shape: shape,
                result_message: msg,
                detail: Vec::new(),
            },
        )
}

fn arb_datalog_term() -> impl Strategy<Value = DatalogTerm> {
    prop_oneof![
        "[xyz]".prop_map(DatalogTerm::Var),
        small_term().prop_map(DatalogTerm::Const),
    ]
}

fn arb_atom() -> impl Strategy<Value = Atom> {
    (
        small_named(),
        prop::collection::vec(arb_datalog_term(), 0..3),
    )
        .prop_map(|(p, ts)| Atom::new(p, ts))
}

fn arb_body_literal() -> impl Strategy<Value = BodyLiteral> {
    prop_oneof![
        arb_atom().prop_map(BodyLiteral::Positive),
        arb_atom().prop_map(BodyLiteral::Negative),
    ]
}

fn arb_rule() -> impl Strategy<Value = Rule> {
    (arb_atom(), prop::collection::vec(arb_body_literal(), 0..3)).prop_map(|(h, b)| Rule::new(h, b))
}

// ---------------------------------------------------------------------------
// SHACL
// ---------------------------------------------------------------------------

proptest! {
    /// SHACL §3.5: `conforms` iff the result set is empty, for any mix of
    /// severities; results are kept verbatim and in order.
    #[test]
    fn shacl_conforms_iff_no_results(results in prop::collection::vec(arb_result(), 0..5)) {
        let r = ShaclValidationReport::from_results(results.clone());
        prop_assert_eq!(r.conforms, results.is_empty());
        prop_assert_eq!(&r.results, &results);
    }

    #[test]
    fn shacl_nested_detail_round_trips(outer in arb_result(), inner in prop::collection::vec(arb_result(), 0..3)) {
        let mut r = outer;
        r.detail = inner.clone();
        let cloned = r.clone();
        prop_assert_eq!(&cloned, &r);
        prop_assert_eq!(cloned.detail, inner);
    }

    #[test]
    fn property_path_clone_eq(p in arb_path()) {
        prop_assert_eq!(p.clone(), p);
    }
}

#[test]
fn shacl_conforming_equals_empty_from_results() {
    assert_eq!(
        ShaclValidationReport::conforming(),
        ShaclValidationReport::from_results(Vec::new())
    );
}

#[test]
fn severities_are_distinct() {
    assert_ne!(Severity::Violation, Severity::Warning);
    assert_ne!(Severity::Warning, Severity::Info);
    assert_ne!(Severity::Violation, Severity::Info);
}

#[test]
fn node_validation_result_holds_fields() {
    let focus: NamedOrBlankNode = NamedNode::new("https://example.org/n").into();
    let nr = NodeValidationResult {
        focus: focus.clone(),
        conforms: true,
        results: Vec::new(),
    };
    assert_eq!(nr.clone(), nr);
    assert_eq!(nr.focus, focus);
}

// ---------------------------------------------------------------------------
// Datalog
// ---------------------------------------------------------------------------

proptest! {
    /// `DatalogResult` agrees with a per-predicate `Vec` reference model:
    /// tuples are kept in insertion order (duplicates included), counts match,
    /// and unseen predicates have no relation.
    #[test]
    fn datalog_result_matches_reference(
        facts in prop::collection::vec((small_named(), prop::collection::vec(small_term(), 0..3)), 0..16),
        probe in small_named(),
    ) {
        let mut r = DatalogResult::new();
        let mut model: HashMap<NamedNode, Vec<Vec<Term>>> = HashMap::new();
        for (p, t) in &facts {
            r.add_fact(p.clone(), t.clone());
            model.entry(p.clone()).or_default().push(t.clone());
        }
        prop_assert_eq!(r.predicate_count(), model.len());
        prop_assert_eq!(r.total_facts(), facts.len());
        for (p, tuples) in &model {
            prop_assert_eq!(r.relation(p), Some(tuples.as_slice()));
        }
        if !model.contains_key(&probe) {
            prop_assert_eq!(r.relation(&probe), None);
        }
    }

    /// Equality ignores how facts for *different* predicates were
    /// interleaved; only per-predicate order matters.
    #[test]
    fn datalog_result_eq_ignores_cross_predicate_interleaving(
        facts in prop::collection::vec((small_named(), prop::collection::vec(small_term(), 0..2)), 0..12),
    ) {
        let mut a = DatalogResult::new();
        for (p, t) in &facts {
            a.add_fact(p.clone(), t.clone());
        }
        // Same facts, grouped by predicate (stable within each predicate).
        let mut grouped: BTreeMap<NamedNode, Vec<Vec<Term>>> = BTreeMap::new();
        for (p, t) in &facts {
            grouped.entry(p.clone()).or_default().push(t.clone());
        }
        let mut b = DatalogResult::default();
        for (p, ts) in grouped.into_iter().rev() {
            for t in ts {
                b.add_fact(p.clone(), t);
            }
        }
        prop_assert_eq!(a, b);
    }

    #[test]
    fn datalog_program_has_negation_iff_any_negative_literal(rules in prop::collection::vec(arb_rule(), 0..5)) {
        let expected = rules.iter().flat_map(|r| &r.body).any(|l| matches!(l, BodyLiteral::Negative(_)));
        let p = DatalogProgram { rules };
        prop_assert_eq!(p.has_negation(), expected);
    }

    #[test]
    fn rule_and_literal_accessors(rule in arb_rule()) {
        prop_assert_eq!(rule.is_fact(), rule.body.is_empty());
        for lit in &rule.body {
            let (atom, neg) = match lit {
                BodyLiteral::Positive(a) => (a, false),
                BodyLiteral::Negative(a) => (a, true),
            };
            prop_assert_eq!(lit.atom(), atom);
            prop_assert_eq!(lit.is_negative(), neg);
        }
        let rebuilt = Rule::new(rule.head.clone(), rule.body.clone());
        prop_assert_eq!(rebuilt, rule);
    }
}

#[test]
fn datalog_program_new_is_empty() {
    let p = DatalogProgram::new();
    assert!(p.rules.is_empty());
    assert!(!p.has_negation());
    assert_eq!(p, DatalogProgram::default());
}

#[test]
fn datalog_term_variants() {
    let v = DatalogTerm::Var("x".into());
    let c = DatalogTerm::Const(Term::Named(NamedNode::new("https://example.org/a")));
    assert_ne!(v, c);
    assert_eq!(c.clone(), c);
}

// ---------------------------------------------------------------------------
// OWL
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn realization_types_of_matches_map(
        entries in prop::collection::btree_map(small_named(), prop::collection::btree_set(small_named(), 0..3), 0..5),
        probe in small_named(),
    ) {
        let r = InstanceRealization { individual_types: entries.clone() };
        prop_assert_eq!(r.types_of(&probe), entries.get(&probe));
        for (k, v) in &entries {
            prop_assert_eq!(r.types_of(k), Some(v));
        }
    }
}

#[test]
fn class_hierarchy_new_is_empty() {
    let h = ClassHierarchy::new();
    assert!(h.equivalence_classes.is_empty());
    assert!(h.direct_subclass_of.is_empty());
    assert!(h.unsatisfiable.is_empty());
    assert_eq!(h, ClassHierarchy::default());
}

#[test]
fn class_hierarchy_holds_fields() {
    // `ClassIri` / `IndividualIri` are plain aliases for `NamedNode`.
    let a: ClassIri = NamedNode::new("https://example.org/A");
    let b: ClassIri = NamedNode::new("https://example.org/B");
    let i: IndividualIri = NamedNode::new("https://example.org/i");
    let h = ClassHierarchy {
        equivalence_classes: vec![EquivalenceClass {
            members: [a.clone(), b.clone()].into_iter().collect(),
        }],
        direct_subclass_of: vec![(a.clone(), b.clone())],
        unsatisfiable: vec![b.clone()],
    };
    assert_eq!(h.clone(), h);
    let r = InstanceRealization {
        individual_types: [(i.clone(), BTreeSet::from([a]))].into_iter().collect(),
    };
    assert!(r.types_of(&i).is_some());
}

#[test]
fn consistency_result_with_explanation() {
    let inc = ConsistencyResult::Inconsistent {
        explanation: Some(vec![Axiom("DisjointClasses(:A :B)".into())]),
    };
    assert!(!inc.is_consistent());
    assert_ne!(inc, ConsistencyResult::Inconsistent { explanation: None });
    assert_ne!(Axiom("a".into()), Axiom("b".into()));
}

// ---------------------------------------------------------------------------
// Storage metadata types
// ---------------------------------------------------------------------------

#[test]
fn storage_capability_variants_are_distinct() {
    let all = [
        StorageCapability::Persistent,
        StorageCapability::ConcurrentWrites,
        StorageCapability::BulkLoad,
        StorageCapability::SparqlEndpoint,
        StorageCapability::ApproximateQuadCount,
        StorageCapability::Isolation(TransactionIsolation::ReadCommitted),
        StorageCapability::Isolation(TransactionIsolation::SnapshotIsolation),
        StorageCapability::Isolation(TransactionIsolation::Serializable),
    ];
    for (i, a) in all.iter().enumerate() {
        for (j, b) in all.iter().enumerate() {
            assert_eq!(i == j, a == b, "{a:?} vs {b:?}");
        }
    }
}

#[test]
fn storage_capabilities_compare_fieldwise() {
    let base = StorageCapabilities {
        persistent: false,
        concurrent_writes: false,
        bulk_load: false,
        sparql_endpoint: false,
        approximate_quad_count: false,
        isolation: TransactionIsolation::ReadCommitted,
    };
    let variants = [
        StorageCapabilities {
            persistent: true,
            ..base
        },
        StorageCapabilities {
            concurrent_writes: true,
            ..base
        },
        StorageCapabilities {
            bulk_load: true,
            ..base
        },
        StorageCapabilities {
            sparql_endpoint: true,
            ..base
        },
        StorageCapabilities {
            approximate_quad_count: true,
            ..base
        },
        StorageCapabilities {
            isolation: TransactionIsolation::Serializable,
            ..base
        },
    ];
    for v in variants {
        assert_ne!(v, base);
    }
}

// ---------------------------------------------------------------------------
// Error Display, one assertion per variant
// ---------------------------------------------------------------------------

fn assert_error_traits<E: std::error::Error + Send + Sync + 'static>() {}

#[test]
fn errors_are_send_sync_static() {
    assert_error_traits::<StorageError>();
    assert_error_traits::<ShaclError>();
    assert_error_traits::<DatalogError>();
    assert_error_traits::<OwlError>();
}

#[test]
fn storage_error_display_every_variant() {
    let io: StorageError = std::io::Error::other("disk gone").into();
    assert_eq!(io.to_string(), "io: disk gone");
    assert!(io.source().is_some(), "Io wraps its cause as source");
    assert_eq!(
        StorageError::Serialization("bad term".into()).to_string(),
        "serialization: bad term"
    );
    assert_eq!(
        StorageError::TransactionConflict.to_string(),
        "transaction conflict"
    );
    assert_eq!(
        StorageError::UnsupportedCapability(StorageCapability::Isolation(
            TransactionIsolation::Serializable
        ))
        .to_string(),
        "unsupported capability: Isolation(Serializable)"
    );
    assert_eq!(
        StorageError::Backend("oxigraph said no".into()).to_string(),
        "backend: oxigraph said no"
    );
}

#[test]
fn shacl_error_display_every_variant() {
    let focus: NamedOrBlankNode = NamedNode::new("https://example.org/n").into();
    let c = ShaclError::ConstraintViolation {
        focus,
        constraint: NamedNode::new("https://example.org/C"),
        message: "nope".into(),
    }
    .to_string();
    assert!(c.starts_with("constraint violation on "), "{c}");
    assert!(c.contains("https://example.org/n") && c.contains("https://example.org/C"));
    assert!(c.ends_with(": nope"));
    assert!(ShaclError::UnsupportedConstraintComponent {
        component: NamedNode::new("https://example.org/X")
    }
    .to_string()
    .contains("https://example.org/X"));
    assert_eq!(
        ShaclError::SparqlExecutionError {
            detail: "parse".into()
        }
        .to_string(),
        "sparql execution error: parse"
    );
    assert_eq!(
        ShaclError::MalformedShapesGraph {
            reason: "cycle".into()
        }
        .to_string(),
        "malformed shapes graph: cycle"
    );
    assert_eq!(
        ShaclError::QuotaExceeded {
            kind: "shapes".into(),
            limit: 7
        }
        .to_string(),
        "quota exceeded: shapes (limit 7)"
    );
    assert_eq!(
        ShaclError::License {
            feature: "Sparql".into()
        }
        .to_string(),
        "feature not licensed: Sparql"
    );
}

#[test]
fn datalog_error_display_every_variant() {
    assert_eq!(
        DatalogError::GroundingFailure {
            rule: "r1".into(),
            reason: "unbound ?x".into()
        }
        .to_string(),
        "grounding failure in rule r1: unbound ?x"
    );
    assert_eq!(
        DatalogError::StratificationViolation {
            rule_set: "p :- not p".into()
        }
        .to_string(),
        "stratification violation: p :- not p"
    );
    assert_eq!(
        DatalogError::EvaluationLimitExceeded { iterations: 9 }.to_string(),
        "evaluation limit exceeded after 9 iterations"
    );
    assert_eq!(
        DatalogError::QuotaExceeded {
            kind: "facts".into(),
            limit: 3
        }
        .to_string(),
        "quota exceeded: facts (limit 3)"
    );
    assert_eq!(
        DatalogError::License {
            feature: "Chase".into()
        }
        .to_string(),
        "feature not licensed: Chase"
    );
}

#[test]
fn owl_error_display_every_variant() {
    let cases = [
        (OwlError::Load { reason: "r".into() }, "owl load error: r"),
        (
            OwlError::UnsupportedAxiom {
                axiom: "a".into(),
                profile: "QL".into(),
            },
            "unsupported axiom for QL profile: a",
        ),
        (
            OwlError::ProfileViolation { reason: "r".into() },
            "profile violation: r",
        ),
        (
            OwlError::ConsistencyViolation { reason: "r".into() },
            "consistency violation: r",
        ),
        (
            OwlError::MaterializationFailure { reason: "r".into() },
            "materialization failure: r",
        ),
        (
            OwlError::EvaluationLimitExceeded { limit: 5 },
            "evaluation limit exceeded: 5",
        ),
        (
            OwlError::QuotaExceeded {
                kind: "axioms".into(),
                limit: 2,
            },
            "quota exceeded: axioms (limit 2)",
        ),
        (
            OwlError::License {
                feature: "OwlDl".into(),
            },
            "feature not licensed: OwlDl",
        ),
    ];
    for (err, expected) in cases {
        assert_eq!(err.to_string(), expected);
    }
}
