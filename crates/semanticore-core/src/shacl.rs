//! The `ShaclValidator` trait and validation-report vocabulary (RFC 0003).
//!
//! Validation produces a [`ShaclValidationReport`], not a graph delta, so
//! `ShaclValidator` is a peer of [`crate::datalog::DatalogEngine`] over the
//! [`crate::reasoner::Reasoner`] base trait rather than a materialization trait.
//! The report types here are the canonical SHACL report vocabulary shared
//! between the engine and any consumer; the premium engine projects them to the
//! DataFrame shape the maplib seam exposes.

use crate::model::{NamedNode, NamedOrBlankNode, Term};
use crate::reasoner::Reasoner;

/// A SHACL validator over a data graph and a shapes graph.
pub trait ShaclValidator: Reasoner {
    /// Validate `data` against `shapes`, producing a SHACL Core conformant
    /// report. `data` and `shapes` are separate stores so deployments can keep
    /// them under different access controls.
    fn validate(
        &self,
        data: &Self::Storage,
        shapes: &Self::Storage,
    ) -> Result<ShaclValidationReport, Self::Error>;

    /// Validate a single focus node — the interactive / incremental path.
    fn validate_node(
        &self,
        data: &Self::Storage,
        shapes: &Self::Storage,
        focus: &NamedOrBlankNode,
    ) -> Result<NodeValidationResult, Self::Error>;

    /// Whether this implementation accepts `sh:sparql` constraints.
    fn supports_sparql_constraints(&self) -> bool;
}

/// The SHACL property-path vocabulary (the 7 path kinds). The canonical form a
/// validation result's `sh:resultPath` is reported against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyPath {
    /// A single predicate.
    Predicate(NamedNode),
    /// The inverse of a path.
    Inverse(Box<PropertyPath>),
    /// A sequence of paths.
    Sequence(Vec<PropertyPath>),
    /// An alternative between paths.
    Alternative(Vec<PropertyPath>),
    /// Zero or more repetitions.
    ZeroOrMore(Box<PropertyPath>),
    /// One or more repetitions.
    OneOrMore(Box<PropertyPath>),
    /// Zero or one.
    ZeroOrOne(Box<PropertyPath>),
}

/// Severity of a validation result (`sh:Violation`/`sh:Warning`/`sh:Info`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// `sh:Violation` (the default).
    Violation,
    /// `sh:Warning`.
    Warning,
    /// `sh:Info`.
    Info,
}

/// A single SHACL validation result. Carries the mandatory W3C fields plus the
/// optional ones; nested `detail` results support SHACL-AF detail traces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    /// The focus node the result is about (`sh:focusNode`).
    pub focus_node: Term,
    /// Severity (`sh:resultSeverity`).
    pub result_severity: Severity,
    /// The constraint component that produced the result
    /// (`sh:sourceConstraintComponent`).
    pub source_constraint_component: NamedNode,
    /// The path the result is about (`sh:resultPath`), for property-shape
    /// results.
    pub result_path: Option<PropertyPath>,
    /// The offending value (`sh:value`).
    pub value: Option<Term>,
    /// The shape that produced the result (`sh:sourceShape`).
    pub source_shape: Option<NamedOrBlankNode>,
    /// A human-readable message (`sh:resultMessage`).
    pub result_message: Option<String>,
    /// Nested detail results (`sh:detail`).
    pub detail: Vec<ValidationResult>,
}

/// A full SHACL validation report (`sh:ValidationReport`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaclValidationReport {
    /// `sh:conforms` — true iff there are no results of severity
    /// [`Severity::Violation`].
    pub conforms: bool,
    /// The validation results (`sh:result`).
    pub results: Vec<ValidationResult>,
}

impl ShaclValidationReport {
    /// A conforming (empty) report.
    pub fn conforming() -> Self {
        Self {
            conforms: true,
            results: Vec::new(),
        }
    }

    /// Build a report from results, deriving `conforms` from whether the result
    /// set is empty. Per SHACL §3.5 a graph conforms iff the validation report
    /// contains **no results** of any severity.
    pub fn from_results(results: Vec<ValidationResult>) -> Self {
        let conforms = results.is_empty();
        Self { conforms, results }
    }
}

/// The result of validating a single focus node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeValidationResult {
    /// The focus node that was validated.
    pub focus: NamedOrBlankNode,
    /// Whether it conforms (no violation-severity results).
    pub conforms: bool,
    /// The results for this focus node.
    pub results: Vec<ValidationResult>,
}

/// The SHACL family error. Most violations are reported in the validation report;
/// these errors are reserved for situations where the validator cannot proceed.
#[derive(Debug, thiserror::Error)]
pub enum ShaclError {
    /// A constraint could not be evaluated (rare; most violations go in the
    /// report).
    #[error("constraint violation on {focus:?} ({constraint:?}): {message}")]
    ConstraintViolation {
        /// The focus node.
        focus: NamedOrBlankNode,
        /// The offending constraint component IRI.
        constraint: NamedNode,
        /// Explanation.
        message: String,
    },
    /// The shapes graph uses a constraint component this validator does not
    /// implement.
    #[error("unsupported constraint component: {component:?}")]
    UnsupportedConstraintComponent {
        /// The unsupported component IRI.
        component: NamedNode,
    },
    /// A `sh:sparql` constraint failed to parse or execute.
    #[error("sparql execution error: {detail}")]
    SparqlExecutionError {
        /// The underlying message.
        detail: String,
    },
    /// The shapes graph is malformed.
    #[error("malformed shapes graph: {reason}")]
    MalformedShapesGraph {
        /// What is wrong.
        reason: String,
    },
    /// A license/quota bound was exceeded.
    #[error("quota exceeded: {kind} (limit {limit})")]
    QuotaExceeded {
        /// The quota kind.
        kind: String,
        /// The limit that was hit.
        limit: u64,
    },
    /// The operation requires a premium feature that is not licensed. Premium
    /// engines map their licensing failure into this variant; OSS code never
    /// produces it. (Carries the feature name as a string so `semanticore-core`
    /// takes no dependency on the premium licensing crate.)
    #[error("feature not licensed: {feature}")]
    License {
        /// The premium feature that was required.
        feature: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(severity: Severity) -> ValidationResult {
        ValidationResult {
            focus_node: Term::Named(NamedNode::new("https://example.org/n")),
            result_severity: severity,
            source_constraint_component: NamedNode::new(
                "http://www.w3.org/ns/shacl#MinCountConstraintComponent",
            ),
            result_path: Some(PropertyPath::Predicate(NamedNode::new(
                "https://example.org/p",
            ))),
            value: None,
            source_shape: None,
            result_message: Some("too few".into()),
            detail: Vec::new(),
        }
    }

    #[test]
    fn conforming_report_is_empty() {
        let r = ShaclValidationReport::conforming();
        assert!(r.conforms);
        assert!(r.results.is_empty());
    }

    #[test]
    fn violation_makes_report_nonconforming() {
        let r = ShaclValidationReport::from_results(vec![result(Severity::Violation)]);
        assert!(!r.conforms);
        assert_eq!(r.results.len(), 1);
    }

    #[test]
    fn warning_only_does_not_conform() {
        // SHA-CONF-sev: SHACL §3.5 — any result → conforms:false, regardless of severity.
        let r = ShaclValidationReport::from_results(vec![result(Severity::Warning)]);
        assert!(!r.conforms);
    }

    // -----------------------------------------------------------------------
    // SHA-CONF-sev: spec-correct conforms logic (FAILING against current code)
    // -----------------------------------------------------------------------
    //
    // SHACL §3.5: "A graph conforms if and only if a validation report produced
    // by validating the graph contains no results."
    //
    // The current implementation computes:
    //   conforms = !results.any(|r| r.result_severity == Severity::Violation)
    // This is WRONG: a Warning/Info-only report should still yield conforms:false.
    //
    // These tests MUST FAIL against the current code and MUST PASS after the fix:
    //   let conforms = results.is_empty();

    /// SHA-CONF-sev: a Warning-severity result must make conforms = false.
    ///
    /// The existing test `warning_only_still_conforms` (above) codifies the BUG.
    /// This test encodes the SPEC-CORRECT behavior.  Both tests coexist until the
    /// fix is applied; at that point `warning_only_still_conforms` will be renamed
    /// to `warning_only_does_not_conform` and its assertion inverted.
    #[test]
    fn sha_conf_warning_result_makes_conforms_false() {
        // SHA-CONF-sev: SHACL §3.5 — any result → conforms:false.
        // Currently from_results returns conforms:true for Warning-only → FAILS here.
        let r = ShaclValidationReport::from_results(vec![result(Severity::Warning)]);
        assert!(
            !r.conforms,
            "SHA-CONF-sev: a Warning-severity result must make conforms = false; \
             SHACL §3.5: conformance = no results of ANY severity. \
             from_results currently returns conforms:true for Warning-only results."
        );
    }

    /// SHA-CONF-sev: an Info-severity result must make conforms = false.
    #[test]
    fn sha_conf_info_result_makes_conforms_false() {
        // SHA-CONF-sev: Info-severity result → conforms must be false.
        // Currently from_results returns conforms:true for Info-only → FAILS here.
        let r = ShaclValidationReport::from_results(vec![result(Severity::Info)]);
        assert!(
            !r.conforms,
            "SHA-CONF-sev: an Info-severity result must make conforms = false; \
             SHACL §3.5: 'a graph conforms iff the validation report has no results'. \
             from_results currently returns conforms:true for Info-only results."
        );
    }

    /// SHA-CONF-sev: a mix of Warning + Info results (no Violations) must make
    /// conforms = false.
    #[test]
    fn sha_conf_mixed_warning_info_makes_conforms_false() {
        // SHA-CONF-sev: two non-Violation results → conforms must be false.
        let r = ShaclValidationReport::from_results(vec![
            result(Severity::Warning),
            result(Severity::Info),
        ]);
        assert!(
            !r.conforms,
            "SHA-CONF-sev: Warning+Info results (no Violation) must make \
             conforms = false; got conforms:true"
        );
    }

    /// SHA-CONF-sev non-regression: empty results still yields conforms = true.
    #[test]
    fn sha_conf_empty_results_conforms_true() {
        // Non-regression: no results → conforms:true.  Must pass before and after fix.
        let r = ShaclValidationReport::from_results(vec![]);
        assert!(
            r.conforms,
            "non-regression SHA-CONF-sev: from_results([]) must yield conforms:true"
        );
    }

    #[test]
    fn shacl_error_displays() {
        let e = ShaclError::UnsupportedConstraintComponent {
            component: NamedNode::new("http://www.w3.org/ns/shacl#ClosedConstraintComponent"),
        };
        assert!(e.to_string().contains("unsupported constraint component"));
    }

    #[test]
    fn property_path_nests() {
        let p = PropertyPath::Inverse(Box::new(PropertyPath::Predicate(NamedNode::new(
            "https://example.org/p",
        ))));
        assert!(matches!(p, PropertyPath::Inverse(_)));
    }
}
