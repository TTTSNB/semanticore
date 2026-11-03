//! The `DatalogEngine` trait, the canonical program IR, and result types
//! (RFC 0003).
//!
//! The program IR here is backend-neutral: it carries no dataframe or columnar
//! types. A parser (supplied by an engine or by the caller) produces a
//! [`DatalogProgram`]; the engine evaluates it against an extensional database
//! read through [`crate::storage::Storage`] and returns a [`DatalogResult`] of
//! derived facts. The trait surface is fixed at the RFC 0003 scope (stratified
//! negation; the [`NegationKind`] enum is forward-compatible with well-founded
//! semantics). Extensions beyond that scope are left to engines as inherent
//! methods.

use std::collections::HashMap;

use crate::model::{NamedNode, Term};
use crate::reasoner::Reasoner;

/// A Datalog engine.
///
/// # Example
///
/// A toy engine for single-atom rule bodies, called through the trait:
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
/// use semanticore_core::datalog::{
///     Atom, BodyLiteral, DatalogEngine, DatalogProgram, DatalogResult, DatalogTerm, NegationKind,
///     Rule,
/// };
/// use semanticore_core::reasoner::{Reasoner, ReasonerCapabilities, ReasonerView};
///
/// /// A toy engine: evaluates non-recursive rules whose body is one positive
/// /// binary atom, reading the EDB as `predicate(subject, object)` quads.
/// #[derive(Default)]
/// struct OneStep {
///     program: DatalogProgram,
/// }
///
/// impl Reasoner for OneStep {
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
/// impl DatalogEngine for OneStep {
///     fn load_program(&mut self, program: &DatalogProgram) -> Result<(), StorageError> {
///         let supported = program.rules.iter().all(|r| {
///             matches!(r.body.as_slice(), [BodyLiteral::Positive(a)] if a.terms.len() == 2)
///         });
///         if !supported {
///             return Err(StorageError::Backend("unsupported rule shape".into()));
///         }
///         self.program = program.clone();
///         Ok(())
///     }
///
///     fn evaluate(&self, edb: &MemStore) -> Result<DatalogResult, StorageError> {
///         let tx = edb.begin_read()?;
///         let mut result = DatalogResult::new();
///         for rule in &self.program.rules {
///             let [BodyLiteral::Positive(body)] = rule.body.as_slice() else { continue };
///             for quad in tx.quads_for_pattern(None, Some(&body.predicate), None, None) {
///                 let quad = quad?;
///                 let row = [Term::from(quad.subject), quad.object];
///                 // Bind body variables to the quad's subject/object.
///                 let lookup = |t: &DatalogTerm| match t {
///                     DatalogTerm::Const(c) => Some(c.clone()),
///                     DatalogTerm::Var(v) => body
///                         .terms
///                         .iter()
///                         .position(|b| matches!(b, DatalogTerm::Var(w) if w == v))
///                         .map(|i| row[i].clone()),
///                 };
///                 let consts_match = body.terms.iter().zip(&row).all(|(t, value)| match t {
///                     DatalogTerm::Const(c) => c == value,
///                     DatalogTerm::Var(_) => true,
///                 });
///                 if let (true, Some(tuple)) =
///                     (consts_match, rule.head.terms.iter().map(lookup).collect::<Option<Vec<_>>>())
///                 {
///                     result.add_fact(rule.head.predicate.clone(), tuple);
///                 }
///             }
///         }
///         Ok(result)
///     }
///
///     fn supports_negation(&self) -> NegationKind {
///         NegationKind::None
///     }
///
///     fn supports_aggregation(&self) -> bool {
///         false
///     }
/// }
///
/// let edb = MemStore::default();
/// load(&edb, &[
///     Quad::triple(ex("alice").into(), ex("parent"), ex("bob").into()),
///     Quad::triple(ex("bob").into(), ex("parent"), ex("carol").into()),
/// ]);
///
/// // child(Y, X) :- parent(X, Y).
/// let var = |n: &str| DatalogTerm::Var(n.into());
/// let mut program = DatalogProgram::new();
/// program.rules.push(Rule::new(
///     Atom::new(ex("child"), vec![var("Y"), var("X")]),
///     vec![BodyLiteral::Positive(Atom::new(ex("parent"), vec![var("X"), var("Y")]))],
/// ));
///
/// let mut engine = OneStep::default();
/// engine.load_program(&program)?;
/// let result = engine.evaluate(&edb)?;
/// assert_eq!(result.total_facts(), 2);
/// let children = result.relation(&ex("child")).unwrap();
/// assert!(children.contains(&vec![ex("bob").into(), ex("alice").into()]));
/// assert_eq!(engine.supports_negation(), NegationKind::None);
/// # Ok::<(), StorageError>(())
/// ```
pub trait DatalogEngine: Reasoner {
    /// Load a program, replacing any previously loaded one. Stratification /
    /// acyclicity checks run here and may reject the program.
    fn load_program(&mut self, program: &DatalogProgram) -> Result<(), Self::Error>;

    /// Evaluate the loaded program against an extensional database, returning the
    /// intensional result.
    fn evaluate(&self, edb: &Self::Storage) -> Result<DatalogResult, Self::Error>;

    /// What kind of negation this engine supports.
    fn supports_negation(&self) -> NegationKind;

    /// Whether this engine supports aggregate atoms.
    fn supports_aggregation(&self) -> bool;
}

/// The kind of negation an engine supports. Forward-compatible: adding
/// `Wellfounded` does not change any caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegationKind {
    /// No negation; pure positive Datalog.
    None,
    /// Stratified negation (the v0.1 scope).
    Stratified,
    /// Well-founded semantics.
    Wellfounded,
}

/// A term in a Datalog rule: a variable or a ground RDF term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatalogTerm {
    /// A variable, identified by name.
    Var(String),
    /// A ground constant (any RDF term).
    Const(Term),
}

/// An atom: a predicate applied to a list of terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atom {
    /// The predicate symbol.
    pub predicate: NamedNode,
    /// The argument terms.
    pub terms: Vec<DatalogTerm>,
}

impl Atom {
    /// Construct an atom.
    pub fn new(predicate: NamedNode, terms: Vec<DatalogTerm>) -> Self {
        Self { predicate, terms }
    }
}

/// A body literal: a positive or negated atom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyLiteral {
    /// A positive atom.
    Positive(Atom),
    /// A negated atom (negation-as-failure).
    Negative(Atom),
}

impl BodyLiteral {
    /// The underlying atom regardless of polarity.
    pub fn atom(&self) -> &Atom {
        match self {
            BodyLiteral::Positive(a) | BodyLiteral::Negative(a) => a,
        }
    }

    /// Whether this literal is negated.
    pub fn is_negative(&self) -> bool {
        matches!(self, BodyLiteral::Negative(_))
    }
}

/// A Datalog rule: a head atom entailed by a conjunction of body literals. A
/// rule with an empty body is a fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// The head atom.
    pub head: Atom,
    /// The body literals (conjunction). Empty for a fact.
    pub body: Vec<BodyLiteral>,
}

impl Rule {
    /// Construct a rule.
    pub fn new(head: Atom, body: Vec<BodyLiteral>) -> Self {
        Self { head, body }
    }

    /// Whether this rule is a fact (empty body).
    pub fn is_fact(&self) -> bool {
        self.body.is_empty()
    }
}

/// A Datalog program: an ordered set of rules.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DatalogProgram {
    /// The rules.
    pub rules: Vec<Rule>,
}

impl DatalogProgram {
    /// An empty program.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether any rule body contains a negated literal.
    pub fn has_negation(&self) -> bool {
        self.rules
            .iter()
            .any(|r| r.body.iter().any(BodyLiteral::is_negative))
    }
}

/// The result of evaluating a program: derived facts grouped by predicate. Each
/// fact is the tuple of ground terms in argument order. Backend-neutral (no
/// dataframe); engines may project it to other shapes outside this crate.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DatalogResult {
    relations: HashMap<NamedNode, Vec<Vec<Term>>>,
}

impl DatalogResult {
    /// An empty result.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a derived fact for `predicate`.
    pub fn add_fact(&mut self, predicate: NamedNode, tuple: Vec<Term>) {
        self.relations.entry(predicate).or_default().push(tuple);
    }

    /// The derived tuples for a predicate, if any.
    pub fn relation(&self, predicate: &NamedNode) -> Option<&[Vec<Term>]> {
        self.relations.get(predicate).map(Vec::as_slice)
    }

    /// The number of distinct predicates with derived facts.
    pub fn predicate_count(&self) -> usize {
        self.relations.len()
    }

    /// The total number of derived facts across all predicates.
    pub fn total_facts(&self) -> usize {
        self.relations.values().map(Vec::len).sum()
    }
}

/// The Datalog family error.
#[derive(Debug, thiserror::Error)]
pub enum DatalogError {
    /// A rule body could not be grounded during evaluation.
    #[error("grounding failure in rule {rule}: {reason}")]
    GroundingFailure {
        /// A description of the offending rule.
        rule: String,
        /// Why grounding failed.
        reason: String,
    },
    /// The program mixes recursion through negation and cannot be stratified.
    #[error("stratification violation: {rule_set}")]
    StratificationViolation {
        /// A description of the offending rule set.
        rule_set: String,
    },
    /// Evaluation exceeded its iteration bound.
    #[error("evaluation limit exceeded after {iterations} iterations")]
    EvaluationLimitExceeded {
        /// The iteration count reached.
        iterations: u64,
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

    fn var(n: &str) -> DatalogTerm {
        DatalogTerm::Var(n.into())
    }

    fn pred(n: &str) -> NamedNode {
        NamedNode::new(format!("https://example.org/{n}"))
    }

    #[test]
    fn fact_has_empty_body() {
        let f = Rule::new(Atom::new(pred("edge"), vec![var("x"), var("y")]), vec![]);
        assert!(f.is_fact());
    }

    #[test]
    fn program_detects_negation() {
        let mut p = DatalogProgram::new();
        p.rules.push(Rule::new(
            Atom::new(pred("reachable"), vec![var("x")]),
            vec![BodyLiteral::Positive(Atom::new(
                pred("edge"),
                vec![var("x")],
            ))],
        ));
        assert!(!p.has_negation());
        p.rules.push(Rule::new(
            Atom::new(pred("unreached"), vec![var("x")]),
            vec![BodyLiteral::Negative(Atom::new(
                pred("reachable"),
                vec![var("x")],
            ))],
        ));
        assert!(p.has_negation());
    }

    #[test]
    fn body_literal_polarity() {
        let lit = BodyLiteral::Negative(Atom::new(pred("p"), vec![var("x")]));
        assert!(lit.is_negative());
        assert_eq!(lit.atom().predicate, pred("p"));
    }

    #[test]
    fn result_accumulates_and_counts() {
        let mut r = DatalogResult::new();
        let p = pred("path");
        r.add_fact(
            p.clone(),
            vec![Term::Named(pred("a")), Term::Named(pred("b"))],
        );
        r.add_fact(
            p.clone(),
            vec![Term::Named(pred("b")), Term::Named(pred("c"))],
        );
        assert_eq!(r.predicate_count(), 1);
        assert_eq!(r.total_facts(), 2);
        assert_eq!(r.relation(&p).unwrap().len(), 2);
        assert!(r.relation(&pred("missing")).is_none());
    }

    #[test]
    fn datalog_error_displays() {
        let e = DatalogError::StratificationViolation {
            rule_set: "p :- not p".into(),
        };
        assert!(e.to_string().contains("stratification violation"));
    }
}
