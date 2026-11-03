//! The `Reasoner` base trait and capability advertisement (RFC 0003).
//!
//! Every reasoner family ([`OwlReasoner`], [`crate::shacl::ShaclValidator`],
//! [`crate::datalog::DatalogEngine`]) builds on the [`Reasoner`] base trait.
//! Capabilities are inert data the Session reads to pick among linked reasoners
//! at runtime; the trait does not enforce them.
//!
//! The traits in this module:
//!
//! - [`Reasoner`] — the base trait: storage type, error type, capabilities.
//! - [`Materialization`] — forward chaining: write entailments to storage.
//! - [`IncrementalReasoner`] — keep a materialization current under updates.
//! - [`Entailment`] — backward chaining: answer entailment queries read-only.
//! - [`OwlReasoner`] — the OWL surface (classification, realization,
//!   consistency); its result types live in [`crate::owl`].
//!
//! Each trait's docs include a compiling toy implementation over a toy
//! in-memory store (the store is shown in full on [`Storage`]).
//!
//! Scope note: the Context reasoner family (`ContextReasoner`) from RFC 0003 is
//! not defined in this release.

use crate::model::Quad;
use crate::storage::Storage;

/// The base trait every reasoner implements.
///
/// `Send + Sync` is non-negotiable: the Context OS holds reasoners as
/// `Arc<dyn Reasoner>` inside async tasks.
///
/// # Example
///
/// A reasoner that only advertises its capabilities, and a caller that routes
/// on them:
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
/// use semanticore_core::reasoner::{
///     OwlProfile, ProfileSet, Reasoner, ReasonerCapabilities, ReasonerView,
/// };
///
/// /// A toy reasoner that only advertises what it can do.
/// struct ToyElReasoner;
///
/// impl Reasoner for ToyElReasoner {
///     type Storage = MemStore; // any `Storage` implementation
///     type Error = StorageError;
///
///     fn capabilities(&self) -> ReasonerCapabilities {
///         ReasonerCapabilities {
///             profiles: ProfileSet::empty().with(OwlProfile::El),
///             incremental: true,
///             ..ReasonerCapabilities::default()
///         }
///     }
///
///     fn over<'a>(&'a self, storage: &'a MemStore) -> ReasonerView<'a, Self> {
///         ReasonerView::new(self, storage)
///     }
/// }
///
/// // A caller (typically the Session) routes on capabilities...
/// let reasoner = ToyElReasoner;
/// let caps = reasoner.capabilities();
/// assert!(caps.profiles.contains(OwlProfile::El));
/// assert!(!caps.profiles.contains(OwlProfile::Dl));
/// assert!(caps.incremental && !caps.distributed);
///
/// // ...and binds the chosen reasoner to a store.
/// let store = MemStore::default();
/// let view = reasoner.over(&store);
/// assert_eq!(view.storage().quad_count()?, 0);
/// # Ok::<(), StorageError>(())
/// ```
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
///
/// # Example
///
/// A toy symmetric-closure materializer, also implementing
/// [`IncrementalReasoner`]:
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
/// use semanticore_core::reasoner::{
///     IncrementalReasoner, Materialization, MaterializationStats, Reasoner,
///     ReasonerCapabilities, ReasonerView,
/// };
///
/// /// A toy forward chainer: writes the converse of every asserted `ex:knows`
/// /// quad into the `ex:inferred` graph.
/// struct SymmetricClosure;
///
/// impl SymmetricClosure {
///     fn inferred() -> NamedOrDefault {
///         NamedOrDefault::Named(ex("inferred"))
///     }
///
///     fn add_converses(
///         &self,
///         storage: &MemStore,
///         asserted: &[Quad],
///     ) -> Result<MaterializationStats, StorageError> {
///         let mut tx = storage.begin_write()?;
///         let mut added = 0;
///         for q in asserted {
///             if let (NamedOrBlankNode::Named(s), Term::Named(o)) = (&q.subject, &q.object) {
///                 let converse = Quad::new(
///                     o.clone().into(),
///                     q.predicate.clone(),
///                     s.clone().into(),
///                     Self::inferred(),
///                 );
///                 let asserted_already = Quad { graph: q.graph.clone(), ..converse.clone() };
///                 if !tx.contains(&asserted_already)? && tx.insert(&converse)? {
///                     added += 1;
///                 }
///             }
///         }
///         tx.commit()?;
///         Ok(MaterializationStats { quads_added: added, quads_removed: 0, iterations: 1, elapsed_ms: 0.0 })
///     }
/// }
///
/// impl Reasoner for SymmetricClosure {
///     type Storage = MemStore;
///     type Error = StorageError;
///
///     fn capabilities(&self) -> ReasonerCapabilities {
///         ReasonerCapabilities { incremental: true, ..ReasonerCapabilities::default() }
///     }
///
///     fn over<'a>(&'a self, storage: &'a MemStore) -> ReasonerView<'a, Self> {
///         ReasonerView::new(self, storage)
///     }
/// }
///
/// impl Materialization for SymmetricClosure {
///     fn materialize(&self, storage: &mut MemStore) -> Result<MaterializationStats, StorageError> {
///         let asserted: Vec<Quad> = storage
///             .begin_read()?
///             .quads_for_pattern(None, Some(&ex("knows")), None, Some(&NamedOrDefault::Default))
///             .collect::<Result<_, _>>()?;
///         self.add_converses(storage, &asserted)
///     }
///
///     fn dematerialize(&self, storage: &mut MemStore) -> Result<(), StorageError> {
///         let mut tx = storage.begin_write()?;
///         let inferred: Vec<Quad> = tx
///             .quads_for_pattern(None, None, None, Some(&Self::inferred()))
///             .collect::<Result<_, _>>()?;
///         for q in &inferred {
///             tx.remove(q)?;
///         }
///         tx.commit()
///     }
/// }
///
/// impl IncrementalReasoner for SymmetricClosure {
///     fn on_insert(
///         &self,
///         storage: &mut MemStore,
///         inserted: &[Quad],
///     ) -> Result<MaterializationStats, StorageError> {
///         let knows: Vec<Quad> =
///             inserted.iter().filter(|q| q.predicate == ex("knows")).cloned().collect();
///         self.add_converses(storage, &knows)
///     }
///
///     fn on_remove(
///         &self,
///         storage: &mut MemStore,
///         _removed: &[Quad],
///     ) -> Result<MaterializationStats, StorageError> {
///         // The always-correct fallback: recompute from the asserted base.
///         self.dematerialize(storage)?;
///         self.materialize(storage)
///     }
/// }
///
/// let mut store = MemStore::default();
/// let ab = Quad::triple(ex("alice").into(), ex("knows"), ex("bob").into());
/// load(&store, &[ab.clone()]);
///
/// let reasoner = SymmetricClosure;
/// let stats = reasoner.materialize(&mut store)?;
/// assert_eq!(stats.quads_added, 1);
/// assert_eq!(store.quad_count()?, 2);
///
/// // Incremental maintenance after the caller inserts a new fact.
/// let bc = Quad::triple(ex("bob").into(), ex("knows"), ex("carol").into());
/// load(&store, &[bc.clone()]);
/// assert_eq!(reasoner.on_insert(&mut store, &[bc])?.quads_added, 1);
/// assert_eq!(store.quad_count()?, 4);
///
/// // Dematerializing leaves only the asserted base.
/// reasoner.dematerialize(&mut store)?;
/// assert_eq!(store.quad_count()?, 2);
/// # Ok::<(), StorageError>(())
/// ```
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
///
/// See [`Materialization`] for an example that implements both traits.
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
///
/// # Example
///
/// The trivial implementation, in which the asserted graph entails exactly
/// itself (the crate-level docs show a less trivial one):
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
/// use semanticore_core::reasoner::{Entailment, Reasoner, ReasonerCapabilities, ReasonerView};
///
/// /// The trivial backward chainer: the asserted graph entails exactly itself.
/// struct Asserted;
///
/// impl Reasoner for Asserted {
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
/// impl Entailment for Asserted {
///     fn entails(&self, storage: &MemStore, candidate: &Quad) -> Result<bool, StorageError> {
///         storage.begin_read()?.contains(candidate)
///     }
///
///     fn entailed_quads<'a>(
///         &'a self,
///         storage: &'a MemStore,
///     ) -> Box<dyn Iterator<Item = Result<Quad, StorageError>> + 'a> {
///         match storage.begin_read() {
///             Ok(tx) => {
///                 let all: Vec<_> = tx.quads_for_pattern(None, None, None, None).collect();
///                 Box::new(all.into_iter())
///             }
///             Err(e) => Box::new(std::iter::once(Err(e))),
///         }
///     }
/// }
///
/// let store = MemStore::default();
/// let q = Quad::triple(ex("alice").into(), ex("knows"), ex("bob").into());
/// load(&store, &[q.clone()]);
///
/// // Entailment never writes to storage.
/// let reasoner = Asserted;
/// assert!(reasoner.entails(&store, &q)?);
/// assert_eq!(reasoner.entailed_quads(&store).count(), 1);
/// assert_eq!(store.quad_count()?, 1);
/// # Ok::<(), StorageError>(())
/// ```
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
///
/// # Example
///
/// A toy that understands only `rdf:type` and `rdfs:subClassOf`, implementing
/// every supertrait so that it can be called through `OwlReasoner`:
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
/// use std::collections::BTreeSet;
///
/// use semanticore_core::owl::{ClassHierarchy, ConsistencyResult, InstanceRealization};
/// use semanticore_core::reasoner::{
///     Entailment, IncrementalReasoner, Materialization, MaterializationStats, OwlProfile,
///     OwlReasoner, ProfileSet, Reasoner, ReasonerCapabilities, ReasonerView,
/// };
///
/// const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
/// const SUB_CLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
///
/// /// A toy "reasoner" that understands only `rdf:type` and `rdfs:subClassOf`
/// /// between named nodes. Real OWL engines live outside this crate.
/// struct ToySubsumption;
///
/// impl ToySubsumption {
///     fn edges(storage: &MemStore, predicate: &str) -> Result<Vec<(NamedNode, NamedNode)>, StorageError> {
///         let tx = storage.begin_read()?;
///         let quads: Vec<Quad> = tx
///             .quads_for_pattern(None, Some(&NamedNode::new(predicate)), None, None)
///             .collect::<Result<_, _>>()?;
///         Ok(quads
///             .into_iter()
///             .filter_map(|q| match (q.subject, q.object) {
///                 (NamedOrBlankNode::Named(s), Term::Named(o)) => Some((s, o)),
///                 _ => None,
///             })
///             .collect())
///     }
///
///     /// Asserted types closed upward under `rdfs:subClassOf`.
///     fn realization(storage: &MemStore) -> Result<InstanceRealization, StorageError> {
///         let sub = Self::edges(storage, SUB_CLASS_OF)?;
///         let mut r = InstanceRealization::new();
///         for (individual, class) in Self::edges(storage, RDF_TYPE)? {
///             let types = r.individual_types.entry(individual).or_default();
///             let mut todo = vec![class];
///             while let Some(c) = todo.pop() {
///                 if types.insert(c.clone()) {
///                     todo.extend(sub.iter().filter(|(s, _)| *s == c).map(|(_, o)| o.clone()));
///                 }
///             }
///         }
///         Ok(r)
///     }
///
///     fn type_quads(storage: &MemStore) -> Result<Vec<Quad>, StorageError> {
///         let r = Self::realization(storage)?;
///         Ok(r.individual_types
///             .into_iter()
///             .flat_map(|(i, types)| {
///                 types.into_iter().map(move |c| {
///                     Quad::triple(i.clone().into(), NamedNode::new(RDF_TYPE), c.into())
///                 })
///             })
///             .collect())
///     }
/// }
///
/// impl Reasoner for ToySubsumption {
///     type Storage = MemStore;
///     type Error = StorageError;
///
///     fn capabilities(&self) -> ReasonerCapabilities {
///         ReasonerCapabilities {
///             profiles: ProfileSet::empty().with(OwlProfile::El),
///             ..ReasonerCapabilities::default()
///         }
///     }
///
///     fn over<'a>(&'a self, storage: &'a MemStore) -> ReasonerView<'a, Self> {
///         ReasonerView::new(self, storage)
///     }
/// }
///
/// impl Materialization for ToySubsumption {
///     fn materialize(&self, storage: &mut MemStore) -> Result<MaterializationStats, StorageError> {
///         let derived = Self::type_quads(storage)?;
///         let mut tx = storage.begin_write()?;
///         let mut added = 0;
///         for q in &derived {
///             added += u64::from(tx.insert(q)?);
///         }
///         tx.commit()?;
///         Ok(MaterializationStats { quads_added: added, quads_removed: 0, iterations: 1, elapsed_ms: 0.0 })
///     }
///
///     fn dematerialize(&self, _storage: &mut MemStore) -> Result<(), StorageError> {
///         // A real engine tracks which quads it derived; the toy keeps none apart.
///         Ok(())
///     }
/// }
///
/// impl IncrementalReasoner for ToySubsumption {
///     fn on_insert(&self, storage: &mut MemStore, _: &[Quad]) -> Result<MaterializationStats, StorageError> {
///         self.materialize(storage) // full recompute is always a conforming fallback
///     }
///
///     fn on_remove(&self, storage: &mut MemStore, _: &[Quad]) -> Result<MaterializationStats, StorageError> {
///         self.dematerialize(storage)?;
///         self.materialize(storage)
///     }
/// }
///
/// impl Entailment for ToySubsumption {
///     fn entails(&self, storage: &MemStore, candidate: &Quad) -> Result<bool, StorageError> {
///         Ok(storage.begin_read()?.contains(candidate)?
///             || Self::type_quads(storage)?.contains(candidate))
///     }
///
///     fn entailed_quads<'a>(
///         &'a self,
///         storage: &'a MemStore,
///     ) -> Box<dyn Iterator<Item = Result<Quad, StorageError>> + 'a> {
///         match Self::type_quads(storage) {
///             Ok(quads) => Box::new(quads.into_iter().map(Ok)),
///             Err(e) => Box::new(std::iter::once(Err(e))),
///         }
///     }
/// }
///
/// impl OwlReasoner for ToySubsumption {
///     fn supported_profiles(&self) -> &[OwlProfile] {
///         &[OwlProfile::El]
///     }
///
///     fn classify(&self, storage: &MemStore) -> Result<ClassHierarchy, StorageError> {
///         let mut h = ClassHierarchy::new();
///         h.direct_subclass_of = Self::edges(storage, SUB_CLASS_OF)?;
///         Ok(h)
///     }
///
///     fn realize(&self, storage: &MemStore) -> Result<InstanceRealization, StorageError> {
///         Self::realization(storage)
///     }
///
///     fn check_consistency(&self, _storage: &MemStore) -> Result<ConsistencyResult, StorageError> {
///         Ok(ConsistencyResult::Consistent) // nothing in this fragment can clash
///     }
/// }
///
/// let store = MemStore::default();
/// load(&store, &[
///     Quad::triple(ex("Cat").into(), NamedNode::new(SUB_CLASS_OF), ex("Animal").into()),
///     Quad::triple(ex("tom").into(), NamedNode::new(RDF_TYPE), ex("Cat").into()),
/// ]);
///
/// let reasoner = ToySubsumption;
/// assert_eq!(reasoner.supported_profiles(), &[OwlProfile::El]);
/// assert_eq!(reasoner.classify(&store)?.direct_subclass_of, vec![(ex("Cat"), ex("Animal"))]);
///
/// let realized = reasoner.realize(&store)?;
/// let expected: BTreeSet<_> = [ex("Animal"), ex("Cat")].into_iter().collect();
/// assert_eq!(realized.types_of(&ex("tom")), Some(&expected));
/// assert!(reasoner.check_consistency(&store)?.is_consistent());
///
/// let tom_is_animal = Quad::triple(ex("tom").into(), NamedNode::new(RDF_TYPE), ex("Animal").into());
/// assert!(reasoner.entails(&store, &tom_is_animal)?);
/// # Ok::<(), StorageError>(())
/// ```
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
