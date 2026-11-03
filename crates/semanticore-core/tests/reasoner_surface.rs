//! Tests for the reasoner trait surface (RFC 0003): every public reasoner
//! trait is implementable from outside the crate, the dyn-compatibility the
//! docs promise actually holds, `ReasonerView` hands back exactly what it was
//! given, and `ProfileSet` behaves as a set.
//!
//! The mock reasoners below are deliberately trivial; the crate defines
//! traits only, so what is under test is the trait surface, not inference.

mod common;

use std::collections::HashSet;
use std::sync::Arc;

use common::MemStore;
use proptest::prelude::*;
use semanticore_core::datalog::{
    DatalogEngine, DatalogError, DatalogProgram, DatalogResult, NegationKind,
};
use semanticore_core::model::{NamedNode, NamedOrBlankNode, Quad};
use semanticore_core::owl::{ClassHierarchy, ConsistencyResult, InstanceRealization, OwlError};
use semanticore_core::reasoner::{
    Entailment, IncrementalReasoner, Materialization, MaterializationStats, OwlProfile,
    OwlReasoner, ProfileSet, Reasoner, ReasonerCapabilities, ReasonerView,
};
use semanticore_core::shacl::{
    NodeValidationResult, ShaclError, ShaclValidationReport, ShaclValidator,
};
use semanticore_core::storage::{Storage, StorageTransaction};

const ZERO_STATS: MaterializationStats = MaterializationStats {
    quads_added: 0,
    quads_removed: 0,
    iterations: 0,
    elapsed_ms: 0.0,
};

// ---------------------------------------------------------------------------
// Mock OWL reasoner: implements Reasoner + Materialization +
// IncrementalReasoner + Entailment + OwlReasoner.
// ---------------------------------------------------------------------------

struct ToyOwl;

impl Reasoner for ToyOwl {
    type Storage = MemStore;
    type Error = OwlError;

    fn capabilities(&self) -> ReasonerCapabilities {
        ReasonerCapabilities {
            profiles: ProfileSet::empty()
                .with(OwlProfile::El)
                .with(OwlProfile::Rl),
            incremental: true,
            ..ReasonerCapabilities::default()
        }
    }

    fn over<'a>(&'a self, storage: &'a MemStore) -> ReasonerView<'a, Self> {
        ReasonerView::new(self, storage)
    }
}

impl Materialization for ToyOwl {
    fn materialize(&self, _: &mut MemStore) -> Result<MaterializationStats, OwlError> {
        Ok(MaterializationStats {
            iterations: 1,
            ..ZERO_STATS
        })
    }

    fn dematerialize(&self, _: &mut MemStore) -> Result<(), OwlError> {
        Ok(())
    }
}

impl IncrementalReasoner for ToyOwl {
    fn on_insert(
        &self,
        _: &mut MemStore,
        inserted: &[Quad],
    ) -> Result<MaterializationStats, OwlError> {
        Ok(MaterializationStats {
            quads_added: inserted.len() as u64,
            ..ZERO_STATS
        })
    }

    fn on_remove(
        &self,
        _: &mut MemStore,
        removed: &[Quad],
    ) -> Result<MaterializationStats, OwlError> {
        Ok(MaterializationStats {
            quads_removed: removed.len() as u64,
            ..ZERO_STATS
        })
    }
}

/// Entailment = membership: the identity entailment regime.
impl Entailment for ToyOwl {
    fn entails(&self, storage: &MemStore, candidate: &Quad) -> Result<bool, OwlError> {
        let tx = storage.begin_read().map_err(|e| OwlError::Load {
            reason: e.to_string(),
        })?;
        tx.contains(candidate).map_err(|e| OwlError::Load {
            reason: e.to_string(),
        })
    }

    fn entailed_quads<'a>(
        &'a self,
        storage: &'a MemStore,
    ) -> Box<dyn Iterator<Item = Result<Quad, OwlError>> + 'a> {
        Box::new(storage.snapshot().into_iter().map(Ok))
    }
}

impl OwlReasoner for ToyOwl {
    fn supported_profiles(&self) -> &[OwlProfile] {
        &[OwlProfile::El, OwlProfile::Rl]
    }

    fn classify(&self, _: &MemStore) -> Result<ClassHierarchy, OwlError> {
        Ok(ClassHierarchy::new())
    }

    fn realize(&self, _: &MemStore) -> Result<InstanceRealization, OwlError> {
        Ok(InstanceRealization::new())
    }

    fn check_consistency(&self, _: &MemStore) -> Result<ConsistencyResult, OwlError> {
        Ok(ConsistencyResult::Consistent)
    }
}

// ---------------------------------------------------------------------------
// Mock SHACL validator and Datalog engine.
// ---------------------------------------------------------------------------

struct ToyShacl;

impl Reasoner for ToyShacl {
    type Storage = MemStore;
    type Error = ShaclError;

    fn capabilities(&self) -> ReasonerCapabilities {
        ReasonerCapabilities::default()
    }

    fn over<'a>(&'a self, storage: &'a MemStore) -> ReasonerView<'a, Self> {
        ReasonerView::new(self, storage)
    }
}

impl ShaclValidator for ToyShacl {
    fn validate(&self, _: &MemStore, _: &MemStore) -> Result<ShaclValidationReport, ShaclError> {
        Ok(ShaclValidationReport::conforming())
    }

    fn validate_node(
        &self,
        _: &MemStore,
        _: &MemStore,
        focus: &NamedOrBlankNode,
    ) -> Result<NodeValidationResult, ShaclError> {
        Ok(NodeValidationResult {
            focus: focus.clone(),
            conforms: true,
            results: Vec::new(),
        })
    }

    fn supports_sparql_constraints(&self) -> bool {
        false
    }
}

#[derive(Default)]
struct ToyDatalog {
    loaded: Option<DatalogProgram>,
}

impl Reasoner for ToyDatalog {
    type Storage = MemStore;
    type Error = DatalogError;

    fn capabilities(&self) -> ReasonerCapabilities {
        ReasonerCapabilities::default()
    }

    fn over<'a>(&'a self, storage: &'a MemStore) -> ReasonerView<'a, Self> {
        ReasonerView::new(self, storage)
    }
}

impl DatalogEngine for ToyDatalog {
    fn load_program(&mut self, program: &DatalogProgram) -> Result<(), DatalogError> {
        if program.has_negation() {
            return Err(DatalogError::StratificationViolation {
                rule_set: "negation unsupported".into(),
            });
        }
        self.loaded = Some(program.clone());
        Ok(())
    }

    fn evaluate(&self, _: &MemStore) -> Result<DatalogResult, DatalogError> {
        Ok(DatalogResult::new())
    }

    fn supports_negation(&self) -> NegationKind {
        NegationKind::None
    }

    fn supports_aggregation(&self) -> bool {
        false
    }
}

fn s(n: &str) -> NamedNode {
    NamedNode::new(format!("https://example.org/{n}"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn reasoner_view_returns_bound_reasoner_and_storage() {
    let r = ToyOwl;
    let store = MemStore::new();
    let view = ReasonerView::new(&r, &store);
    assert!(std::ptr::eq(view.reasoner(), &r));
    assert!(std::ptr::eq(view.storage(), &store));

    let via_over = r.over(&store);
    assert!(std::ptr::eq(via_over.reasoner(), &r));
    assert!(std::ptr::eq(via_over.storage(), &store));
}

/// `Reasoner` docs: "the Context OS holds reasoners as `Arc<dyn Reasoner>`".
/// `Entailment` docs: "keeps the trait object-safe so a Session can hold
/// `dyn Entailment`". This test only compiles if both hold, and likewise for
/// every other family trait.
#[test]
fn reasoner_traits_are_dyn_compatible() {
    let r: Arc<dyn Reasoner<Storage = MemStore, Error = OwlError>> = Arc::new(ToyOwl);
    assert!(r.capabilities().profiles.contains(OwlProfile::El));

    let mut store = MemStore::with_quads(&[Quad::triple(s("a").into(), s("p"), s("b").into())]);
    let q = Quad::triple(s("a").into(), s("p"), s("b").into());

    let e: Box<dyn Entailment<Storage = MemStore, Error = OwlError>> = Box::new(ToyOwl);
    assert!(e.entails(&store, &q).unwrap());
    let all: HashSet<Quad> = e.entailed_quads(&store).map(Result::unwrap).collect();
    assert_eq!(all, store.snapshot());

    let m: Box<dyn Materialization<Storage = MemStore, Error = OwlError>> = Box::new(ToyOwl);
    assert_eq!(m.materialize(&mut store).unwrap().iterations, 1);
    m.dematerialize(&mut store).unwrap();

    let i: Box<dyn IncrementalReasoner<Storage = MemStore, Error = OwlError>> = Box::new(ToyOwl);
    assert_eq!(
        i.on_insert(&mut store, std::slice::from_ref(&q))
            .unwrap()
            .quads_added,
        1
    );
    assert_eq!(
        i.on_remove(&mut store, std::slice::from_ref(&q))
            .unwrap()
            .quads_removed,
        1
    );

    let o: Box<dyn OwlReasoner<Storage = MemStore, Error = OwlError>> = Box::new(ToyOwl);
    assert_eq!(o.supported_profiles(), &[OwlProfile::El, OwlProfile::Rl]);
    assert_eq!(o.classify(&store).unwrap(), ClassHierarchy::new());
    assert_eq!(o.realize(&store).unwrap(), InstanceRealization::new());
    assert!(o.check_consistency(&store).unwrap().is_consistent());

    let v: Box<dyn ShaclValidator<Storage = MemStore, Error = ShaclError>> = Box::new(ToyShacl);
    assert!(v.validate(&store, &store).unwrap().conforms);
    let focus: NamedOrBlankNode = s("a").into();
    let nr = v.validate_node(&store, &store, &focus).unwrap();
    assert_eq!(nr.focus, focus);
    assert!(nr.conforms && nr.results.is_empty());
    assert!(!v.supports_sparql_constraints());

    let mut d: Box<dyn DatalogEngine<Storage = MemStore, Error = DatalogError>> =
        Box::new(ToyDatalog::default());
    d.load_program(&DatalogProgram::new()).unwrap();
    assert_eq!(d.evaluate(&store).unwrap(), DatalogResult::new());
    assert_eq!(d.supports_negation(), NegationKind::None);
    assert!(!d.supports_aggregation());
}

#[test]
fn reasoners_are_send_sync() {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    assert_send_sync::<dyn Reasoner<Storage = MemStore, Error = OwlError>>();
    assert_send_sync::<ToyOwl>();
    assert_send_sync::<ToyShacl>();
    assert_send_sync::<ToyDatalog>();
}

#[test]
fn reasoner_capabilities_default_is_conservative() {
    let c = ReasonerCapabilities::default();
    assert_eq!(
        c,
        ReasonerCapabilities {
            profiles: ProfileSet::empty(),
            incremental: false,
            persistent: false,
            distributed: false,
            explanation: false,
            max_axioms: None,
        }
    );
    let copied = c; // Copy
    assert_eq!(copied, c);
    let custom = ReasonerCapabilities {
        max_axioms: Some(10),
        ..c
    };
    assert_ne!(custom, c);
}

#[test]
fn materialization_stats_are_copy_and_comparable() {
    let a = MaterializationStats {
        quads_added: 3,
        quads_removed: 1,
        iterations: 2,
        elapsed_ms: 1.5,
    };
    let b = a; // Copy
    assert_eq!(a, b);
    assert_ne!(a, ZERO_STATS);
}

#[test]
fn negation_kinds_are_distinct() {
    let all = [
        NegationKind::None,
        NegationKind::Stratified,
        NegationKind::Wellfounded,
    ];
    for (i, a) in all.iter().enumerate() {
        for (j, b) in all.iter().enumerate() {
            assert_eq!(i == j, a == b);
        }
    }
}

fn arb_profile() -> impl Strategy<Value = OwlProfile> {
    prop_oneof![
        Just(OwlProfile::El),
        Just(OwlProfile::Ql),
        Just(OwlProfile::Rl),
        Just(OwlProfile::Dl),
    ]
}

const ALL_PROFILES: [OwlProfile; 4] = [
    OwlProfile::El,
    OwlProfile::Ql,
    OwlProfile::Rl,
    OwlProfile::Dl,
];

proptest! {
    /// `ProfileSet` agrees with a `Vec`-backed reference set under any
    /// sequence of inserts, via both `with` and `insert`.
    #[test]
    fn profile_set_matches_reference(ops in prop::collection::vec(arb_profile(), 0..12)) {
        let mut by_with = ProfileSet::empty();
        let mut by_insert = ProfileSet::default();
        let mut model: Vec<OwlProfile> = Vec::new();
        for p in &ops {
            by_with = by_with.with(*p);
            by_insert.insert(*p);
            if !model.contains(p) {
                model.push(*p);
            }
        }
        prop_assert_eq!(by_with, by_insert);
        prop_assert_eq!(by_with.is_empty(), model.is_empty());
        for p in ALL_PROFILES {
            prop_assert_eq!(by_with.contains(p), model.contains(&p));
        }
    }

    /// Insert is idempotent and order-independent.
    #[test]
    fn profile_set_idempotent_and_commutative(ops in prop::collection::vec(arb_profile(), 0..12)) {
        let fwd = ops.iter().fold(ProfileSet::empty(), |s, p| s.with(*p));
        let rev = ops.iter().rev().fold(ProfileSet::empty(), |s, p| s.with(*p));
        let twice = ops.iter().fold(fwd, |s, p| s.with(*p));
        prop_assert_eq!(fwd, rev);
        prop_assert_eq!(fwd, twice);
    }
}
