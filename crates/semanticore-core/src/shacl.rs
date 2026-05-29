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
    pub focus_node: NamedOrBlankNode,
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

    /// Build a report from results, deriving `conforms` from the presence of any
    /// [`Severity::Violation`].
    pub fn from_results(results: Vec<ValidationResult>) -> Self {
        let conforms = !results
            .iter()
            .any(|r| r.result_severity == Severity::Violation);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(severity: Severity) -> ValidationResult {
        ValidationResult {
            focus_node: NamedOrBlankNode::Named(NamedNode::new("https://example.org/n")),
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
    fn warning_only_still_conforms() {
        let r = ShaclValidationReport::from_results(vec![result(Severity::Warning)]);
        assert!(r.conforms);
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
