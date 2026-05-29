//! The `DatalogEngine` trait, the canonical program IR, and result types
//! (RFC 0003).
//!
//! The program IR here is backend-neutral: it carries no Polars/Arrow types. A
//! pluggable parser (in the premium engine) produces a [`DatalogProgram`]; the
//! engine evaluates it against an extensional database read through
//! [`crate::storage::Storage`] and returns a [`DatalogResult`] of derived facts.
//! The trait surface is fixed at the RFC 0003 scope (stratified negation; the
//! `NegationKind` enum is forward-compatible for well-founded); existential
//! rules / the chase and dual-mode materialization are layered by the premium
//! engine as concrete methods pending the RFC 0003 amendment.

use std::collections::HashMap;

use crate::model::{NamedNode, Term};
use crate::reasoner::Reasoner;

/// A Datalog engine.
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
/// DataFrame); the premium engine projects this to per-predicate frames.
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
