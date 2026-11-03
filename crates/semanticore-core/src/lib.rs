//! Shared RDF types and the storage and reasoner traits for semantiCore.
//!
//! `semanticore-core` is the trait layer of the semantiCore ecosystem. It
//! **defines** the data model and the trait surfaces that storage backends and
//! reasoning engines implement; it ships **no implementations** of its own. Open
//! source defaults and commercial engines both depend on this crate and
//! implement its traits, so application code written against these traits does
//! not change when the engine behind them does.
//!
//! # Trait families
//!
//! | Module | Traits | Purpose |
//! |---|---|---|
//! | [`model`] | none (data types) | RDF terms: [`NamedNode`](model::NamedNode), [`BlankNode`](model::BlankNode), [`Literal`](model::Literal), [`Term`](model::Term), [`Quad`](model::Quad). |
//! | [`storage`] | [`Storage`](storage::Storage), [`StorageTransaction`](storage::StorageTransaction) | A transactional quad store. Every read and write goes through a transaction. |
//! | [`reasoner`] | [`Reasoner`](reasoner::Reasoner), [`Materialization`](reasoner::Materialization), [`IncrementalReasoner`](reasoner::IncrementalReasoner), [`Entailment`](reasoner::Entailment), [`OwlReasoner`](reasoner::OwlReasoner) | The base reasoner trait, capability advertisement, forward- and backward-chaining surfaces, and the OWL surface. |
//! | [`owl`] | none (result types) | Return and error types for [`OwlReasoner`](reasoner::OwlReasoner). |
//! | [`shacl`] | [`ShaclValidator`](shacl::ShaclValidator) | SHACL validation and the validation-report vocabulary. |
//! | [`datalog`] | [`DatalogEngine`](datalog::DatalogEngine) | Datalog evaluation over a backend-neutral program IR. |
//!
//! Every reasoner family extends [`Reasoner`](reasoner::Reasoner), which names
//! the [`Storage`](storage::Storage) type the reasoner reads from and an error
//! type. Capabilities ([`ReasonerCapabilities`](reasoner::ReasonerCapabilities),
//! [`StorageCapabilities`](storage::StorageCapabilities)) are plain data that
//! callers read to choose among linked implementations.
//!
//! # Implementing a backend
//!
//! 1. **Storage.** Implement [`StorageTransaction`](storage::StorageTransaction)
//!    for your transaction type (insert, remove, contains, pattern matching,
//!    commit, rollback), then [`Storage`](storage::Storage) for the store
//!    itself (begin read/write transactions, counts, named graphs,
//!    capabilities). The transaction type may borrow from the store. You can
//!    use [`StorageError`](storage::StorageError) or your own error type. See
//!    the [`Storage`](storage::Storage) docs for a complete in-memory example.
//! 2. **Reasoner.** Implement [`Reasoner`](reasoner::Reasoner): choose the
//!    `Storage` type, choose an error type, advertise capabilities, and return
//!    a [`ReasonerView`](reasoner::ReasonerView) from `over`.
//! 3. **Family trait.** Implement the surface your engine provides:
//!    [`Materialization`](reasoner::Materialization) (and optionally
//!    [`IncrementalReasoner`](reasoner::IncrementalReasoner)) for forward
//!    chaining, [`Entailment`](reasoner::Entailment) for backward chaining,
//!    [`OwlReasoner`](reasoner::OwlReasoner),
//!    [`ShaclValidator`](shacl::ShaclValidator), or
//!    [`DatalogEngine`](datalog::DatalogEngine). Each trait's docs include a
//!    compiling toy implementation.
//!
//! # Example
//!
//! A toy [`Entailment`](reasoner::Entailment) reasoner over a toy in-memory
//! store (the store is hidden here; it is shown in full on
//! [`Storage`](storage::Storage)). The reasoner treats `ex:knows` as
//! symmetric:
//!
//! ```
//! # use std::sync::Mutex;
//! #
//! # use semanticore_core::model::{NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term};
//! # use semanticore_core::storage::{
//! #     Storage, StorageCapabilities, StorageError, StorageTransaction, TransactionIsolation,
//! # };
//! #
//! # /// A toy in-memory store: one `Vec<Quad>` behind a mutex.
//! # #[derive(Default)]
//! # struct MemStore {
//! #     quads: Mutex<Vec<Quad>>,
//! # }
//! #
//! # /// A transaction that stages writes locally and applies them on commit.
//! # struct MemTx<'a> {
//! #     store: &'a MemStore,
//! #     writable: bool,
//! #     inserted: Vec<Quad>,
//! #     removed: Vec<Quad>,
//! # }
//! #
//! # impl MemTx<'_> {
//! #     /// The committed quads with this transaction's staged writes applied.
//! #     fn snapshot(&self) -> Vec<Quad> {
//! #         let base = self.store.quads.lock().unwrap();
//! #         base.iter()
//! #             .filter(|q| !self.removed.contains(q))
//! #             .chain(self.inserted.iter())
//! #             .cloned()
//! #             .collect()
//! #     }
//! #
//! #     fn check_writable(&self) -> Result<(), StorageError> {
//! #         if self.writable {
//! #             Ok(())
//! #         } else {
//! #             Err(StorageError::Backend("read-only transaction".into()))
//! #         }
//! #     }
//! # }
//! #
//! # impl StorageTransaction for MemTx<'_> {
//! #     type Error = StorageError;
//! #
//! #     fn insert(&mut self, quad: &Quad) -> Result<bool, StorageError> {
//! #         self.check_writable()?;
//! #         if self.contains(quad)? {
//! #             return Ok(false);
//! #         }
//! #         match self.removed.iter().position(|q| q == quad) {
//! #             Some(i) => {
//! #                 self.removed.swap_remove(i);
//! #             }
//! #             None => self.inserted.push(quad.clone()),
//! #         }
//! #         Ok(true)
//! #     }
//! #
//! #     fn remove(&mut self, quad: &Quad) -> Result<bool, StorageError> {
//! #         self.check_writable()?;
//! #         if !self.contains(quad)? {
//! #             return Ok(false);
//! #         }
//! #         match self.inserted.iter().position(|q| q == quad) {
//! #             Some(i) => {
//! #                 self.inserted.swap_remove(i);
//! #             }
//! #             None => self.removed.push(quad.clone()),
//! #         }
//! #         Ok(true)
//! #     }
//! #
//! #     fn contains(&self, quad: &Quad) -> Result<bool, StorageError> {
//! #         Ok(self.snapshot().contains(quad))
//! #     }
//! #
//! #     fn quads_for_pattern<'a>(
//! #         &'a self,
//! #         s: Option<&NamedOrBlankNode>,
//! #         p: Option<&NamedNode>,
//! #         o: Option<&Term>,
//! #         g: Option<&NamedOrDefault>,
//! #     ) -> Box<dyn Iterator<Item = Result<Quad, StorageError>> + 'a> {
//! #         let (s, p, o, g) = (s.cloned(), p.cloned(), o.cloned(), g.cloned());
//! #         // A real backend streams from an index; the toy filters a snapshot.
//! #         Box::new(
//! #             self.snapshot()
//! #                 .into_iter()
//! #                 .filter(move |q| {
//! #                     s.as_ref().is_none_or(|s| &q.subject == s)
//! #                         && p.as_ref().is_none_or(|p| &q.predicate == p)
//! #                         && o.as_ref().is_none_or(|o| &q.object == o)
//! #                         && g.as_ref().is_none_or(|g| &q.graph == g)
//! #                 })
//! #                 .map(Ok),
//! #         )
//! #     }
//! #
//! #     fn commit(self) -> Result<(), StorageError> {
//! #         let store = self.store;
//! #         let mut base = store.quads.lock().unwrap();
//! #         base.retain(|q| !self.removed.contains(q));
//! #         base.extend(self.inserted);
//! #         Ok(())
//! #     }
//! #
//! #     fn rollback(self) -> Result<(), StorageError> {
//! #         Ok(()) // Staged writes are simply dropped.
//! #     }
//! # }
//! #
//! # impl Storage for MemStore {
//! #     type Transaction<'a> = MemTx<'a>;
//! #     type Error = StorageError;
//! #
//! #     fn begin_read(&self) -> Result<MemTx<'_>, StorageError> {
//! #         Ok(MemTx { store: self, writable: false, inserted: Vec::new(), removed: Vec::new() })
//! #     }
//! #
//! #     fn begin_write(&self) -> Result<MemTx<'_>, StorageError> {
//! #         Ok(MemTx { store: self, writable: true, inserted: Vec::new(), removed: Vec::new() })
//! #     }
//! #
//! #     fn quad_count(&self) -> Result<u64, StorageError> {
//! #         Ok(self.quads.lock().unwrap().len() as u64)
//! #     }
//! #
//! #     fn named_graphs(&self) -> Result<Vec<NamedNode>, StorageError> {
//! #         let mut graphs: Vec<NamedNode> = self
//! #             .quads
//! #             .lock()
//! #             .unwrap()
//! #             .iter()
//! #             .filter_map(|q| match &q.graph {
//! #                 NamedOrDefault::Named(g) => Some(g.clone()),
//! #                 NamedOrDefault::Default => None,
//! #             })
//! #             .collect();
//! #         graphs.sort();
//! #         graphs.dedup();
//! #         Ok(graphs)
//! #     }
//! #
//! #     fn capabilities(&self) -> StorageCapabilities {
//! #         StorageCapabilities {
//! #             persistent: false,
//! #             concurrent_writes: false,
//! #             bulk_load: false,
//! #             sparql_endpoint: false,
//! #             approximate_quad_count: false,
//! #             isolation: TransactionIsolation::ReadCommitted,
//! #         }
//! #     }
//! # }
//! #
//! # /// Insert `quads` in one committed write transaction.
//! # fn load(store: &MemStore, quads: &[Quad]) {
//! #     let mut tx = store.begin_write().unwrap();
//! #     for q in quads {
//! #         tx.insert(q).unwrap();
//! #     }
//! #     tx.commit().unwrap();
//! # }
//! #
//! # /// Shorthand for an `https://example.org/` IRI.
//! # fn ex(local: &str) -> NamedNode {
//! #     NamedNode::new(format!("https://example.org/{local}"))
//! # }
//! use semanticore_core::reasoner::{
//!     Entailment, Reasoner, ReasonerCapabilities, ReasonerView,
//! };
//!
//! /// A toy reasoner: `ex:knows` is symmetric, so `a knows b` entails
//! /// `b knows a`.
//! struct SymmetricKnows;
//!
//! impl Reasoner for SymmetricKnows {
//!     type Storage = MemStore; // any `Storage` implementation
//!     type Error = StorageError;
//!
//!     fn capabilities(&self) -> ReasonerCapabilities {
//!         ReasonerCapabilities::default()
//!     }
//!
//!     fn over<'a>(&'a self, storage: &'a MemStore) -> ReasonerView<'a, Self> {
//!         ReasonerView::new(self, storage)
//!     }
//! }
//!
//! impl Entailment for SymmetricKnows {
//!     fn entails(&self, storage: &MemStore, candidate: &Quad) -> Result<bool, StorageError> {
//!         let tx = storage.begin_read()?;
//!         if tx.contains(candidate)? {
//!             return Ok(true);
//!         }
//!         // Only reasons about `ex:knows` between named nodes.
//!         let (NamedOrBlankNode::Named(s), Term::Named(o)) = (&candidate.subject, &candidate.object)
//!         else {
//!             return Ok(false);
//!         };
//!         if candidate.predicate != ex("knows") {
//!             return Ok(false);
//!         }
//!         let flipped = Quad::new(
//!             o.clone().into(),
//!             ex("knows"),
//!             s.clone().into(),
//!             candidate.graph.clone(),
//!         );
//!         tx.contains(&flipped)
//!     }
//!
//!     fn entailed_quads<'a>(
//!         &'a self,
//!         storage: &'a MemStore,
//!     ) -> Box<dyn Iterator<Item = Result<Quad, StorageError>> + 'a> {
//!         let tx = match storage.begin_read() {
//!             Ok(tx) => tx,
//!             Err(e) => return Box::new(std::iter::once(Err(e))),
//!         };
//!         let asserted: Vec<Quad> = tx
//!             .quads_for_pattern(None, Some(&ex("knows")), None, None)
//!             .collect::<Result<_, _>>()
//!             .unwrap_or_default();
//!         Box::new(asserted.into_iter().filter_map(|q| match (q.subject, q.object) {
//!             (NamedOrBlankNode::Named(s), Term::Named(o)) => {
//!                 Some(Ok(Quad::new(o.into(), q.predicate, s.into(), q.graph)))
//!             }
//!             _ => None,
//!         }))
//!     }
//! }
//!
//! // Assert one fact through the storage traits...
//! let store = MemStore::default();
//! let alice_knows_bob = Quad::triple(ex("alice").into(), ex("knows"), ex("bob").into());
//! load(&store, &[alice_knows_bob]);
//!
//! // ...then ask the reasoner, through the trait, about the converse.
//! let bob_knows_alice = Quad::triple(ex("bob").into(), ex("knows"), ex("alice").into());
//! let reasoner = SymmetricKnows;
//! assert!(reasoner.entails(&store, &bob_knows_alice)?);
//! assert_eq!(reasoner.entailed_quads(&store).count(), 1);
//!
//! // The Session-facing view binds a reasoner to one store.
//! let view = reasoner.over(&store);
//! assert_eq!(view.storage().quad_count()?, 1);
//! assert!(!view.reasoner().capabilities().distributed);
//! # Ok::<(), StorageError>(())
//! ```
//!
//! # Status
//!
//! This crate is pre-1.0 and its API may change between minor versions. The
//! Context reasoner family described in the project RFCs is not defined here
//! yet. Design documents live in the
//! [repository](https://github.com/TTTSNB/semanticore) under `docs/rfcs`.

#![doc(html_root_url = "https://docs.rs/semanticore-core/0.1.0")]
#![deny(missing_docs)]
#![doc(test(attr(deny(warnings), allow(dead_code))))]

pub mod datalog;
pub mod model;
pub mod owl;
pub mod reasoner;
pub mod shacl;
pub mod storage;

/// An IRI. Retained from the initial scaffold; new code should prefer
/// [`model::NamedNode`] for IRI-named RDF nodes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Iri(String);

impl Iri {
    /// Construct an IRI.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The IRI as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iri_round_trip() {
        let iri = Iri::new("https://example.org/foo");
        assert_eq!(iri.as_str(), "https://example.org/foo");
    }
}
