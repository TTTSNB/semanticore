//! The `ShaclValidator` trait and validation-report vocabulary (RFC 0003).
//!
//! Validation produces a [`ShaclValidationReport`], not a graph delta, so
//! `ShaclValidator` is a peer of [`crate::datalog::DatalogEngine`] over the
//! [`crate::reasoner::Reasoner`] base trait rather than a materialization trait.
//! The report types here are the canonical SHACL report vocabulary shared
//! between any engine and its consumers; engines may additionally project them
//! to other shapes (for example tabular output) outside this crate.

use crate::model::{NamedNode, NamedOrBlankNode, Term};
use crate::reasoner::Reasoner;

/// A SHACL validator over a data graph and a shapes graph.
///
/// # Example
///
/// A toy validator for a single, flattened "at least one value" rule, called
/// through the trait:
///
/// ```
/// # use std::sync::Mutex;
/// #
/// # use semanticore_core::model::{NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term};
/// # use semanticore_core::storage::{
/// #     Storage, StorageCapabilities, StorageError, StorageTransaction, TransactionIsolation,
/// # };
/// #
/// # /// A toy in-memory store: one `Vec<Quad>` behind a mutex.
/// # #[derive(Default)]
/// # struct MemStore {
/// #     quads: Mutex<Vec<Quad>>,
/// # }
/// #
/// # /// A transaction that stages writes locally and applies them on commit.
/// # struct MemTx<'a> {
/// #     store: &'a MemStore,
/// #     writable: bool,
/// #     inserted: Vec<Quad>,
/// #     removed: Vec<Quad>,
/// # }
/// #
/// # impl MemTx<'_> {
/// #     /// The committed quads with this transaction's staged writes applied.
/// #     fn snapshot(&self) -> Vec<Quad> {
/// #         let base = self.store.quads.lock().unwrap();
/// #         base.iter()
/// #             .filter(|q| !self.removed.contains(q))
/// #             .chain(self.inserted.iter())
/// #             .cloned()
/// #             .collect()
/// #     }
/// #
/// #     fn check_writable(&self) -> Result<(), StorageError> {
/// #         if self.writable {
/// #             Ok(())
/// #         } else {
/// #             Err(StorageError::Backend("read-only transaction".into()))
/// #         }
/// #     }
/// # }
/// #
/// # impl StorageTransaction for MemTx<'_> {
/// #     type Error = StorageError;
/// #
/// #     fn insert(&mut self, quad: &Quad) -> Result<bool, StorageError> {
/// #         self.check_writable()?;
/// #         if self.contains(quad)? {
/// #             return Ok(false);
/// #         }
/// #         match self.removed.iter().position(|q| q == quad) {
/// #             Some(i) => {
/// #                 self.removed.swap_remove(i);
/// #             }
/// #             None => self.inserted.push(quad.clone()),
/// #         }
/// #         Ok(true)
/// #     }
/// #
/// #     fn remove(&mut self, quad: &Quad) -> Result<bool, StorageError> {
/// #         self.check_writable()?;
/// #         if !self.contains(quad)? {
/// #             return Ok(false);
/// #         }
/// #         match self.inserted.iter().position(|q| q == quad) {
/// #             Some(i) => {
/// #                 self.inserted.swap_remove(i);
/// #             }
/// #             None => self.removed.push(quad.clone()),
/// #         }
/// #         Ok(true)
/// #     }
/// #
/// #     fn contains(&self, quad: &Quad) -> Result<bool, StorageError> {
/// #         Ok(self.snapshot().contains(quad))
/// #     }
/// #
/// #     fn quads_for_pattern<'a>(
/// #         &'a self,
/// #         s: Option<&NamedOrBlankNode>,
/// #         p: Option<&NamedNode>,
/// #         o: Option<&Term>,
/// #         g: Option<&NamedOrDefault>,
/// #     ) -> Box<dyn Iterator<Item = Result<Quad, StorageError>> + 'a> {
/// #         let (s, p, o, g) = (s.cloned(), p.cloned(), o.cloned(), g.cloned());
/// #         // A real backend streams from an index; the toy filters a snapshot.
/// #         Box::new(
/// #             self.snapshot()
/// #                 .into_iter()
/// #                 .filter(move |q| {
/// #                     s.as_ref().is_none_or(|s| &q.subject == s)
/// #                         && p.as_ref().is_none_or(|p| &q.predicate == p)
/// #                         && o.as_ref().is_none_or(|o| &q.object == o)
/// #                         && g.as_ref().is_none_or(|g| &q.graph == g)
/// #                 })
/// #                 .map(Ok),
/// #         )
/// #     }
/// #
/// #     fn commit(self) -> Result<(), StorageError> {
/// #         let store = self.store;
/// #         let mut base = store.quads.lock().unwrap();
/// #         base.retain(|q| !self.removed.contains(q));
/// #         base.extend(self.inserted);
/// #         Ok(())
/// #     }
/// #
/// #     fn rollback(self) -> Result<(), StorageError> {
/// #         Ok(()) // Staged writes are simply dropped.
/// #     }
/// # }
/// #
/// # impl Storage for MemStore {
/// #     type Transaction<'a> = MemTx<'a>;
/// #     type Error = StorageError;
/// #
/// #     fn begin_read(&self) -> Result<MemTx<'_>, StorageError> {
/// #         Ok(MemTx { store: self, writable: false, inserted: Vec::new(), removed: Vec::new() })
/// #     }
/// #
/// #     fn begin_write(&self) -> Result<MemTx<'_>, StorageError> {
/// #         Ok(MemTx { store: self, writable: true, inserted: Vec::new(), removed: Vec::new() })
/// #     }
/// #
/// #     fn quad_count(&self) -> Result<u64, StorageError> {
/// #         Ok(self.quads.lock().unwrap().len() as u64)
/// #     }
/// #
/// #     fn named_graphs(&self) -> Result<Vec<NamedNode>, StorageError> {
/// #         let mut graphs: Vec<NamedNode> = self
/// #             .quads
/// #             .lock()
/// #             .unwrap()
/// #             .iter()
/// #             .filter_map(|q| match &q.graph {
/// #                 NamedOrDefault::Named(g) => Some(g.clone()),
/// #                 NamedOrDefault::Default => None,
/// #             })
/// #             .collect();
/// #         graphs.sort();
/// #         graphs.dedup();
/// #         Ok(graphs)
/// #     }
/// #
/// #     fn capabilities(&self) -> StorageCapabilities {
/// #         StorageCapabilities {
/// #             persistent: false,
/// #             concurrent_writes: false,
/// #             bulk_load: false,
/// #             sparql_endpoint: false,
/// #             approximate_quad_count: false,
/// #             isolation: TransactionIsolation::ReadCommitted,
/// #         }
/// #     }
/// # }
/// #
/// # /// Insert `quads` in one committed write transaction.
/// # fn load(store: &MemStore, quads: &[Quad]) {
/// #     let mut tx = store.begin_write().unwrap();
/// #     for q in quads {
/// #         tx.insert(q).unwrap();
/// #     }
/// #     tx.commit().unwrap();
/// # }
/// #
/// # /// Shorthand for an `https://example.org/` IRI.
/// # fn ex(local: &str) -> NamedNode {
/// #     NamedNode::new(format!("https://example.org/{local}"))
/// # }
/// use semanticore_core::model::Literal;
/// use semanticore_core::reasoner::{Reasoner, ReasonerCapabilities, ReasonerView};
/// use semanticore_core::shacl::{
///     NodeValidationResult, PropertyPath, Severity, ShaclValidationReport, ShaclValidator,
///     ValidationResult,
/// };
///
/// const SH: &str = "http://www.w3.org/ns/shacl#";
/// const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
///
/// fn sh(local: &str) -> NamedNode {
///     NamedNode::new(format!("{SH}{local}"))
/// }
///
/// /// A toy validator. It reads a deliberately flattened shapes graph
/// /// (`?shape sh:targetClass ?c` and `?shape sh:path ?p`) and checks
/// /// "every instance of `?c` has at least one `?p`". It is not SHACL Core.
/// struct MinCountOne;
///
/// impl MinCountOne {
///     fn objects(tx: &MemTx<'_>, s: &NamedOrBlankNode, p: &NamedNode) -> Result<Vec<Term>, StorageError> {
///         tx.quads_for_pattern(Some(s), Some(p), None, None)
///             .map(|q| q.map(|q| q.object))
///             .collect()
///     }
///
///     fn check(
///         data: &MemStore,
///         shapes: &MemStore,
///         only: Option<&NamedOrBlankNode>,
///     ) -> Result<Vec<ValidationResult>, StorageError> {
///         let (data, shapes) = (data.begin_read()?, shapes.begin_read()?);
///         let mut results = Vec::new();
///         for target in shapes.quads_for_pattern(None, Some(&sh("targetClass")), None, None) {
///             let target = target?;
///             let shape = target.subject;
///             for path in Self::objects(&shapes, &shape, &sh("path"))? {
///                 let Term::Named(path) = path else { continue };
///                 for member in data.quads_for_pattern(None, Some(&NamedNode::new(RDF_TYPE)), Some(&target.object), None) {
///                     let focus = member?.subject;
///                     if only.is_some_and(|f| *f != focus) {
///                         continue;
///                     }
///                     if Self::objects(&data, &focus, &path)?.is_empty() {
///                         results.push(ValidationResult {
///                             focus_node: focus.into(),
///                             result_severity: Severity::Violation,
///                             source_constraint_component: sh("MinCountConstraintComponent"),
///                             result_path: Some(PropertyPath::Predicate(path.clone())),
///                             value: None,
///                             source_shape: Some(shape.clone()),
///                             result_message: Some("missing required value".into()),
///                             detail: Vec::new(),
///                         });
///                     }
///                 }
///             }
///         }
///         Ok(results)
///     }
/// }
///
/// impl Reasoner for MinCountOne {
///     type Storage = MemStore;
///     type Error = StorageError;
///
///     fn capabilities(&self) -> ReasonerCapabilities {
///         ReasonerCapabilities::default()
///     }
///
///     fn over<'a>(&'a self, storage: &'a MemStore) -> ReasonerView<'a, Self> {
///         ReasonerView::new(self, storage)
///     }
/// }
///
/// impl ShaclValidator for MinCountOne {
///     fn validate(&self, data: &MemStore, shapes: &MemStore) -> Result<ShaclValidationReport, StorageError> {
///         Ok(ShaclValidationReport::from_results(Self::check(data, shapes, None)?))
///     }
///
///     fn validate_node(
///         &self,
///         data: &MemStore,
///         shapes: &MemStore,
///         focus: &NamedOrBlankNode,
///     ) -> Result<NodeValidationResult, StorageError> {
///         let results = Self::check(data, shapes, Some(focus))?;
///         Ok(NodeValidationResult { focus: focus.clone(), conforms: results.is_empty(), results })
///     }
///
///     fn supports_sparql_constraints(&self) -> bool {
///         false
///     }
/// }
///
/// // Shapes and data live in separate stores.
/// let shapes = MemStore::default();
/// load(&shapes, &[
///     Quad::triple(ex("PersonShape").into(), sh("targetClass"), ex("Person").into()),
///     Quad::triple(ex("PersonShape").into(), sh("path"), ex("name").into()),
/// ]);
/// let data = MemStore::default();
/// let is_person = |who: &str| Quad::triple(ex(who).into(), NamedNode::new(RDF_TYPE), ex("Person").into());
/// load(&data, &[
///     is_person("alice"),
///     Quad::triple(ex("alice").into(), ex("name"), Literal::new_simple("Alice").into()),
///     is_person("bob"), // no name
/// ]);
///
/// let validator = MinCountOne;
/// let report = validator.validate(&data, &shapes)?;
/// assert!(!report.conforms);
/// assert_eq!(report.results.len(), 1);
/// assert_eq!(report.results[0].focus_node, Term::from(ex("bob")));
///
/// assert!(validator.validate_node(&data, &shapes, &ex("alice").into())?.conforms);
/// assert!(!validator.supports_sparql_constraints());
/// # Ok::<(), StorageError>(())
/// ```
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
    /// `sh:conforms` — true iff the report has no results of any severity.
    /// Per SHACL §3.5 conformance requires an empty result set regardless of
    /// whether results are [`Severity::Violation`], [`Severity::Warning`], or
    /// [`Severity::Info`].
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
    /// Whether the focus node conforms — true iff no results were produced for
    /// it, of any severity (SHACL §3.5).
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
