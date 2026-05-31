//! OWL reasoning return types (RFC 0003 / RFC 0009).
//!
//! These are the OSS-side result types of [`crate::reasoner::OwlReasoner`]
//! (`classify` / `realize` / `check_consistency`). They are backend-neutral and
//! carry no premium type: the hand-rolled OWL axiom IR lives in the premium
//! `semanticore-reasoner-owl-core` crate; the OSS core only needs the opaque
//! [`Axiom`] carrier so the (deferred) explanation surface does not leak a
//! premium type through a trait signature.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::NamedNode;

/// A class IRI. Type alias keeps the surface readable without a new newtype.
pub type ClassIri = NamedNode;
/// An individual IRI.
pub type IndividualIri = NamedNode;

/// The inferred class hierarchy. Preserves equivalence classes rather than
/// canonicalizing, so consumers decide whether to pick a representative.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassHierarchy {
    /// Groups of mutually-subsuming classes (C1 ≡ C2 ≡ …).
    pub equivalence_classes: Vec<EquivalenceClass>,
    /// Direct subclass relations (transitive-reduced, redundancy removed).
    pub direct_subclass_of: Vec<(ClassIri, ClassIri)>,
}

impl ClassHierarchy {
    /// An empty hierarchy.
    pub fn new() -> Self {
        Self::default()
    }
}

/// One equivalence class of mutually-equivalent class IRIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquivalenceClass {
    /// The mutually-equivalent members.
    pub members: BTreeSet<ClassIri>,
}

/// For each named individual, the set of class IRIs it is (asserted or derived to
/// be) an instance of.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstanceRealization {
    /// Individual IRI → derived + asserted types.
    pub individual_types: BTreeMap<IndividualIri, BTreeSet<ClassIri>>,
}

impl InstanceRealization {
    /// An empty realization.
    pub fn new() -> Self {
        Self::default()
    }

    /// The derived + asserted types of `individual`, if any are known.
    pub fn types_of(&self, individual: &IndividualIri) -> Option<&BTreeSet<ClassIri>> {
        self.individual_types.get(individual)
    }
}

/// The result of a consistency check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsistencyResult {
    /// The ontology is consistent.
    Consistent,
    /// The ontology is inconsistent.
    Inconsistent {
        /// Optional justification axioms. `None` at v0.1; RFC 0009 graduates
        /// proof traces.
        explanation: Option<Vec<Axiom>>,
    },
}

impl ConsistencyResult {
    /// Whether the ontology is consistent.
    pub fn is_consistent(&self) -> bool {
        matches!(self, ConsistencyResult::Consistent)
    }
}

/// Opaque OSS-side axiom handle for explanation traces. The full hand-rolled
/// axiom IR lives in `semanticore-reasoner-owl-core`; the OSS core only needs an
/// opaque, comparable carrier so the (deferred) explanation surface does not leak
/// a premium type through a trait return. v0.1 explanations are always empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Axiom(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    fn cls(s: &str) -> NamedNode {
        NamedNode::new(format!("https://example.org/{s}"))
    }

    #[test]
    fn empty_hierarchy_and_realization() {
        assert!(ClassHierarchy::new().equivalence_classes.is_empty());
        assert!(InstanceRealization::new().individual_types.is_empty());
    }

    #[test]
    fn realization_types_of() {
        let mut r = InstanceRealization::new();
        let mut types = BTreeSet::new();
        types.insert(cls("Person"));
        r.individual_types.insert(cls("alice"), types);
        assert_eq!(r.types_of(&cls("alice")).unwrap().len(), 1);
        assert!(r.types_of(&cls("bob")).is_none());
    }

    #[test]
    fn consistency_result_predicate() {
        assert!(ConsistencyResult::Consistent.is_consistent());
        let inc = ConsistencyResult::Inconsistent { explanation: None };
        assert!(!inc.is_consistent());
    }

    #[test]
    fn equivalence_class_holds_members() {
        let ec = EquivalenceClass {
            members: [cls("A"), cls("B")].into_iter().collect(),
        };
        assert_eq!(ec.members.len(), 2);
    }

    #[test]
    fn axiom_is_comparable() {
        assert_eq!(
            Axiom("SubClassOf(:A :B)".into()),
            Axiom("SubClassOf(:A :B)".into())
        );
    }
}
