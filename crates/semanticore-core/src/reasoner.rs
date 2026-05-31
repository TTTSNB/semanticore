//! The `Reasoner` base trait and capability advertisement (RFC 0003).
//!
//! Every reasoner family ([`crate::shacl::ShaclValidator`],
//! [`crate::datalog::DatalogEngine`], and the deferred OWL/Context families)
//! builds on the [`Reasoner`] base trait. Capabilities are inert data the
//! Session reads to pick among linked reasoners at runtime; the trait does not
//! enforce them.
//!
//! Scope note: this module defines the base trait plus the SHACL and Datalog
//! surfaces needed by the premium reasoner engines. The OWL (`OwlReasoner`) and
//! Context (`ContextReasoner`) families from RFC 0003, and the
//! `Materialization`/`IncrementalReasoner`/`Entailment` mix-ins, are deferred to
//! the remainder of Phase 3 and intentionally not defined here.

use crate::model::Quad;
use crate::storage::Storage;

/// The base trait every reasoner implements.
///
/// `Send + Sync` is non-negotiable: the Context OS holds reasoners as
/// `Arc<dyn Reasoner>` inside async tasks.
pub trait Reasoner: Send + Sync {
    /// The storage backend this reasoner operates over (RFC 0002).
    type Storage: Storage;
    /// The family-specific error type.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Capabilities this reasoner advertises, read by the Session for routing.
    fn capabilities(&self) -> ReasonerCapabilities;

    /// Bind this reasoner to a storage instance, producing a view that scopes
    /// subsequent operations to that storage.
    fn over<'a>(&'a self, storage: &'a Self::Storage) -> ReasonerView<'a, Self>
    where
        Self: Sized;
}

/// A reasoner bound to a particular storage instance.
pub struct ReasonerView<'a, R: Reasoner> {
    reasoner: &'a R,
    storage: &'a R::Storage,
}

impl<'a, R: Reasoner> ReasonerView<'a, R> {
    /// Bind `reasoner` to `storage`.
    pub fn new(reasoner: &'a R, storage: &'a R::Storage) -> Self {
        Self { reasoner, storage }
    }

    /// The bound reasoner.
    pub fn reasoner(&self) -> &R {
        self.reasoner
    }

    /// The bound storage.
    pub fn storage(&self) -> &R::Storage {
        self.storage
    }
}

/// What an implementation declares about itself. The Session reads this to route
/// requests. Inert data — misadvertising is a contract violation detected only
/// when an operation fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReasonerCapabilities {
    /// OWL profiles supported. Empty for non-OWL reasoners.
    pub profiles: ProfileSet,
    /// Supports incremental update.
    pub incremental: bool,
    /// Can spill to disk and resume across process restarts. OSS defaults
    /// `false`; premium impls `true`.
    pub persistent: bool,
    /// Can run across processes or machines. OSS defaults `false`; premium
    /// engines `true`.
    pub distributed: bool,
    /// Can return a proof/explanation trace. Deferred to v1.1; defaults `false`.
    pub explanation: bool,
    /// Hard upper bound on input axioms, if any.
    pub max_axioms: Option<u64>,
}

impl Default for ReasonerCapabilities {
    /// The conservative OSS-scope default: no OWL profiles, single-process,
    /// in-memory, no explanation.
    fn default() -> Self {
        Self {
            profiles: ProfileSet::empty(),
            incremental: false,
            persistent: false,
            distributed: false,
            explanation: false,
            max_axioms: None,
        }
    }
}

/// The four OWL 2 profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwlProfile {
    /// EL: subsumption/classification on large bio-medical ontologies.
    El,
    /// QL: query rewriting over relational data.
    Ql,
    /// RL: rule-based; expressible in Datalog.
    Rl,
    /// DL: full OWL 2 DL via tableaux.
    Dl,
}

impl OwlProfile {
    const fn bit(self) -> u8 {
        match self {
            OwlProfile::El => 1 << 0,
            OwlProfile::Ql => 1 << 1,
            OwlProfile::Rl => 1 << 2,
            OwlProfile::Dl => 1 << 3,
        }
    }
}

/// A small bitset over [`OwlProfile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProfileSet(u8);

impl ProfileSet {
    /// The empty set.
    pub const fn empty() -> Self {
        ProfileSet(0)
    }

    /// Return a copy with `profile` added.
    pub const fn with(self, profile: OwlProfile) -> Self {
        ProfileSet(self.0 | profile.bit())
    }

    /// Add `profile` in place.
    pub fn insert(&mut self, profile: OwlProfile) {
        self.0 |= profile.bit();
    }

    /// Whether `profile` is present.
    pub const fn contains(self, profile: OwlProfile) -> bool {
        self.0 & profile.bit() != 0
    }

    /// Whether no profiles are present.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// A reasoner that computes the entailed graph and writes it back to storage —
/// the forward-chaining surface. Dominant pattern for OWL EL/RL and Datalog
/// (RFC 0003).
pub trait Materialization: Reasoner {
    /// Compute all entailments and persist them to storage.
    fn materialize(&self, storage: &mut Self::Storage)
        -> Result<MaterializationStats, Self::Error>;

    /// Remove all materialized entailments, leaving the asserted base.
    fn dematerialize(&self, storage: &mut Self::Storage) -> Result<(), Self::Error>;
}

/// Statistics returned by a materialization pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaterializationStats {
    /// Quads added to storage.
    pub quads_added: u64,
    /// Quads removed (rare; a few rule fires can retract).
    pub quads_removed: u64,
    /// Fixpoint iterations run.
    pub iterations: u32,
    /// Wall-clock elapsed, milliseconds.
    pub elapsed_ms: f64,
}

/// A reasoner that maintains the entailed graph under updates without redoing the
/// full materialization. Extends [`Materialization`] because incremental update
/// implies a baseline to update against (a conforming impl may always fall back
/// to `dematerialize` + `materialize`).
pub trait IncrementalReasoner: Materialization {
    /// Update materialization to reflect newly inserted quads. The caller must
    /// have already inserted them into storage.
    fn on_insert(
        &self,
        storage: &mut Self::Storage,
        inserted: &[Quad],
    ) -> Result<MaterializationStats, Self::Error>;

    /// Update materialization to reflect removed quads. The caller must have
    /// already removed them from the asserted base.
    fn on_remove(
        &self,
        storage: &mut Self::Storage,
        removed: &[Quad],
    ) -> Result<MaterializationStats, Self::Error>;
}

/// A reasoner that answers entailment queries without writing to storage — the
/// backward-chaining surface. The natural surface for OWL QL (query rewriting)
/// and OWL DL (tableaux), and a secondary surface for EL/RL.
pub trait Entailment: Reasoner {
    /// Does the asserted graph entail the given quad?
    fn entails(&self, storage: &Self::Storage, candidate: &Quad) -> Result<bool, Self::Error>;

    /// Iterate over all entailed quads. Implementations may stream lazily;
    /// `Box<dyn …>` keeps the trait object-safe so a Session can hold
    /// `dyn Entailment`.
    fn entailed_quads<'a>(
        &'a self,
        storage: &'a Self::Storage,
    ) -> Box<dyn Iterator<Item = Result<Quad, Self::Error>> + 'a>;
}

/// The OWL reasoning surface. Every OWL engine (EL/QL/RL/DL) implements this; the
/// sound-and-complete subset of forward vs backward differs per profile and is
/// documented per engine. Peer of [`crate::shacl::ShaclValidator`] and
/// [`crate::datalog::DatalogEngine`] over the [`Reasoner`] base trait.
pub trait OwlReasoner: Materialization + IncrementalReasoner + Entailment {
    /// Profiles this reasoner can be asked to operate under.
    fn supported_profiles(&self) -> &[OwlProfile];

    /// Compute the class hierarchy (subClassOf closure with equivalence classes
    /// collapsed).
    fn classify(&self, storage: &Self::Storage) -> Result<crate::owl::ClassHierarchy, Self::Error>;

    /// Compute, for each named individual, its asserted and derived types.
    fn realize(
        &self,
        storage: &Self::Storage,
    ) -> Result<crate::owl::InstanceRealization, Self::Error>;

    /// Check global consistency.
    fn check_consistency(
        &self,
        storage: &Self::Storage,
    ) -> Result<crate::owl::ConsistencyResult, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_set_empty_by_default() {
        let p = ProfileSet::empty();
        assert!(p.is_empty());
        assert!(!p.contains(OwlProfile::El));
    }

    #[test]
    fn profile_set_insert_and_contains() {
        let p = ProfileSet::empty()
            .with(OwlProfile::El)
            .with(OwlProfile::Rl);
        assert!(p.contains(OwlProfile::El));
        assert!(p.contains(OwlProfile::Rl));
        assert!(!p.contains(OwlProfile::Dl));
        assert!(!p.is_empty());
    }

    #[test]
    fn profile_set_insert_mut() {
        let mut p = ProfileSet::empty();
        p.insert(OwlProfile::Ql);
        assert!(p.contains(OwlProfile::Ql));
    }

    #[test]
    fn default_capabilities_are_oss_scope() {
        let c = ReasonerCapabilities::default();
        assert!(!c.persistent);
        assert!(!c.distributed);
        assert!(!c.explanation);
        assert!(c.profiles.is_empty());
        assert_eq!(c.max_axioms, None);
    }
}
