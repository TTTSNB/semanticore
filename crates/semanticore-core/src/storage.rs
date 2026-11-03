//! The `Storage` and `StorageTransaction` traits (RFC 0002).
//!
//! Every operation that reads or writes quads goes through a transaction. The
//! [`Storage`] trait itself only exposes transaction creation, store-level
//! metadata, and capability advertisement. Reasoners take `&impl Storage` and
//! are written against this contract alone, so the backend (Oxigraph in v0.1;
//! GraphDB/Neptune/Blazegraph later) is a runtime choice, not a compile-time
//! entanglement.

use crate::model::{NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term};

/// A transactional quad store.
///
/// `Send + Sync` is mandatory: reasoners and Context engines share the store
/// across threads. The `Transaction<'a>` GAT lets a transaction borrow from the
/// store without forcing `Arc`-based reference counting on every implementation.
///
/// # Implementing
///
/// A backend implements [`StorageTransaction`] for its transaction type and
/// `Storage` for the store. The example below is a complete (if naive)
/// in-memory backend: a `Vec<Quad>` behind a mutex, with writes staged in the
/// transaction and applied on commit. The same `MemStore` is used, hidden, in
/// the reasoner examples elsewhere in this crate.
///
/// # Example
///
/// ```
/// use std::sync::Mutex;
///
/// use semanticore_core::model::{NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term};
/// use semanticore_core::storage::{
///     Storage, StorageCapabilities, StorageError, StorageTransaction, TransactionIsolation,
/// };
///
/// /// A toy in-memory store: one `Vec<Quad>` behind a mutex.
/// #[derive(Default)]
/// struct MemStore {
///     quads: Mutex<Vec<Quad>>,
/// }
///
/// /// A transaction that stages writes locally and applies them on commit.
/// struct MemTx<'a> {
///     store: &'a MemStore,
///     writable: bool,
///     inserted: Vec<Quad>,
///     removed: Vec<Quad>,
/// }
///
/// impl MemTx<'_> {
///     /// The committed quads with this transaction's staged writes applied.
///     fn snapshot(&self) -> Vec<Quad> {
///         let base = self.store.quads.lock().unwrap();
///         base.iter()
///             .filter(|q| !self.removed.contains(q))
///             .chain(self.inserted.iter())
///             .cloned()
///             .collect()
///     }
///
///     fn check_writable(&self) -> Result<(), StorageError> {
///         if self.writable {
///             Ok(())
///         } else {
///             Err(StorageError::Backend("read-only transaction".into()))
///         }
///     }
/// }
///
/// impl StorageTransaction for MemTx<'_> {
///     type Error = StorageError;
///
///     fn insert(&mut self, quad: &Quad) -> Result<bool, StorageError> {
///         self.check_writable()?;
///         if self.contains(quad)? {
///             return Ok(false);
///         }
///         match self.removed.iter().position(|q| q == quad) {
///             Some(i) => {
///                 self.removed.swap_remove(i);
///             }
///             None => self.inserted.push(quad.clone()),
///         }
///         Ok(true)
///     }
///
///     fn remove(&mut self, quad: &Quad) -> Result<bool, StorageError> {
///         self.check_writable()?;
///         if !self.contains(quad)? {
///             return Ok(false);
///         }
///         match self.inserted.iter().position(|q| q == quad) {
///             Some(i) => {
///                 self.inserted.swap_remove(i);
///             }
///             None => self.removed.push(quad.clone()),
///         }
///         Ok(true)
///     }
///
///     fn contains(&self, quad: &Quad) -> Result<bool, StorageError> {
///         Ok(self.snapshot().contains(quad))
///     }
///
///     fn quads_for_pattern<'a>(
///         &'a self,
///         s: Option<&NamedOrBlankNode>,
///         p: Option<&NamedNode>,
///         o: Option<&Term>,
///         g: Option<&NamedOrDefault>,
///     ) -> Box<dyn Iterator<Item = Result<Quad, StorageError>> + 'a> {
///         let (s, p, o, g) = (s.cloned(), p.cloned(), o.cloned(), g.cloned());
///         // A real backend streams from an index; the toy filters a snapshot.
///         Box::new(
///             self.snapshot()
///                 .into_iter()
///                 .filter(move |q| {
///                     s.as_ref().is_none_or(|s| &q.subject == s)
///                         && p.as_ref().is_none_or(|p| &q.predicate == p)
///                         && o.as_ref().is_none_or(|o| &q.object == o)
///                         && g.as_ref().is_none_or(|g| &q.graph == g)
///                 })
///                 .map(Ok),
///         )
///     }
///
///     fn commit(self) -> Result<(), StorageError> {
///         let store = self.store;
///         let mut base = store.quads.lock().unwrap();
///         base.retain(|q| !self.removed.contains(q));
///         base.extend(self.inserted);
///         Ok(())
///     }
///
///     fn rollback(self) -> Result<(), StorageError> {
///         Ok(()) // Staged writes are simply dropped.
///     }
/// }
///
/// impl Storage for MemStore {
///     type Transaction<'a> = MemTx<'a>;
///     type Error = StorageError;
///
///     fn begin_read(&self) -> Result<MemTx<'_>, StorageError> {
///         Ok(MemTx { store: self, writable: false, inserted: Vec::new(), removed: Vec::new() })
///     }
///
///     fn begin_write(&self) -> Result<MemTx<'_>, StorageError> {
///         Ok(MemTx { store: self, writable: true, inserted: Vec::new(), removed: Vec::new() })
///     }
///
///     fn quad_count(&self) -> Result<u64, StorageError> {
///         Ok(self.quads.lock().unwrap().len() as u64)
///     }
///
///     fn named_graphs(&self) -> Result<Vec<NamedNode>, StorageError> {
///         let mut graphs: Vec<NamedNode> = self
///             .quads
///             .lock()
///             .unwrap()
///             .iter()
///             .filter_map(|q| match &q.graph {
///                 NamedOrDefault::Named(g) => Some(g.clone()),
///                 NamedOrDefault::Default => None,
///             })
///             .collect();
///         graphs.sort();
///         graphs.dedup();
///         Ok(graphs)
///     }
///
///     fn capabilities(&self) -> StorageCapabilities {
///         StorageCapabilities {
///             persistent: false,
///             concurrent_writes: false,
///             bulk_load: false,
///             sparql_endpoint: false,
///             approximate_quad_count: false,
///             isolation: TransactionIsolation::ReadCommitted,
///         }
///     }
/// }
///
/// /// Insert `quads` in one committed write transaction.
/// fn load(store: &MemStore, quads: &[Quad]) {
///     let mut tx = store.begin_write().unwrap();
///     for q in quads {
///         tx.insert(q).unwrap();
///     }
///     tx.commit().unwrap();
/// }
///
/// /// Shorthand for an `https://example.org/` IRI.
/// fn ex(local: &str) -> NamedNode {
///     NamedNode::new(format!("https://example.org/{local}"))
/// }
///
/// // Writes go through a write transaction and become visible on commit.
/// let store = MemStore::default();
/// let graph = NamedOrDefault::Named(ex("g"));
/// let q = Quad::new(ex("alice").into(), ex("knows"), ex("bob").into(), graph.clone());
/// let mut tx = store.begin_write()?;
/// assert!(tx.insert(&q)?); // newly added
/// assert!(!tx.insert(&q)?); // already present: no state change
/// tx.commit()?;
/// assert_eq!(store.quad_count()?, 1);
/// assert_eq!(store.named_graphs()?, vec![ex("g")]);
///
/// // Reads use a read transaction and pattern matching (`None` = wildcard).
/// let read = store.begin_read()?;
/// let hits: Vec<Quad> = read
///     .quads_for_pattern(None, Some(&ex("knows")), None, Some(&graph))
///     .collect::<Result<_, _>>()?;
/// assert_eq!(hits, vec![q.clone()]);
///
/// // Read-only transactions reject writes with an error rather than panicking.
/// let mut read = store.begin_read()?;
/// assert!(read.insert(&q).is_err());
///
/// // Rolled-back writes never reach the store.
/// let mut tx = store.begin_write()?;
/// assert!(tx.remove(&q)?);
/// tx.rollback()?;
/// assert_eq!(store.quad_count()?, 1);
/// assert!(!store.capabilities().persistent);
/// # Ok::<(), StorageError>(())
/// ```
pub trait Storage: Send + Sync {
    /// The transaction type produced by [`Storage::begin_read`] /
    /// [`Storage::begin_write`].
    type Transaction<'a>: StorageTransaction
    where
        Self: 'a;
    /// The error type for store-level operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Begin a read-only transaction.
    fn begin_read(&self) -> Result<Self::Transaction<'_>, Self::Error>;

    /// Begin a read-write transaction.
    fn begin_write(&self) -> Result<Self::Transaction<'_>, Self::Error>;

    /// Total number of quads. May be exact or approximate; see
    /// [`StorageCapabilities::approximate_quad_count`].
    fn quad_count(&self) -> Result<u64, Self::Error>;

    /// The named graphs present in the store.
    fn named_graphs(&self) -> Result<Vec<NamedNode>, Self::Error>;

    /// What this backend supports.
    fn capabilities(&self) -> StorageCapabilities;
}

/// Operations available within a single transaction.
///
/// Read-only transactions (from [`Storage::begin_read`]) must reject `insert` and
/// `remove` with `Self::Error` rather than panicking. `commit` and `rollback`
/// consume `self`, so a transaction cannot be used after it terminates.
///
/// See [`Storage`] for a complete example implementation.
pub trait StorageTransaction {
    /// The error type for transaction operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Insert a quad. Returns `true` if the store's state changed (the quad was
    /// newly added).
    fn insert(&mut self, quad: &Quad) -> Result<bool, Self::Error>;

    /// Remove a quad. Returns `true` if the store's state changed (the quad was
    /// present and got removed).
    fn remove(&mut self, quad: &Quad) -> Result<bool, Self::Error>;

    /// Whether the quad is present (honoring read-your-writes within the
    /// transaction).
    fn contains(&self, quad: &Quad) -> Result<bool, Self::Error>;

    /// Lazily iterate quads matching a pattern. `None` in any position matches
    /// anything there. Implementations must stream (not materialize a `Vec`).
    fn quads_for_pattern<'a>(
        &'a self,
        s: Option<&NamedOrBlankNode>,
        p: Option<&NamedNode>,
        o: Option<&Term>,
        g: Option<&NamedOrDefault>,
    ) -> Box<dyn Iterator<Item = Result<Quad, Self::Error>> + 'a>;

    /// Commit the transaction. May be slow (flush/fsync/network).
    fn commit(self) -> Result<(), Self::Error>;

    /// Roll the transaction back. Expected to be cheap.
    fn rollback(self) -> Result<(), Self::Error>;
}

/// What a backend supports. Reasoners check this before issuing capability-gated
/// operations and refuse with [`StorageError::UnsupportedCapability`] otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageCapabilities {
    /// Spills to disk and survives process restarts.
    pub persistent: bool,
    /// Supports concurrent writers.
    pub concurrent_writes: bool,
    /// Exposes a fast bulk-load ingest path.
    pub bulk_load: bool,
    /// Can absorb a full SPARQL query string and return results.
    pub sparql_endpoint: bool,
    /// `quad_count` is O(1) approximate rather than O(n) exact.
    pub approximate_quad_count: bool,
    /// The strongest transaction isolation offered.
    pub isolation: TransactionIsolation,
}

/// Transaction isolation level (advertised in [`StorageCapabilities`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionIsolation {
    /// Read-committed.
    ReadCommitted,
    /// Snapshot isolation (Oxigraph's level).
    SnapshotIsolation,
    /// Serializable.
    Serializable,
}

/// The enum form of a capability, used in capability requests and error
/// variants. Kept in sync with [`StorageCapabilities`] field-for-field by
/// convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageCapability {
    /// Corresponds to [`StorageCapabilities::persistent`].
    Persistent,
    /// Corresponds to [`StorageCapabilities::concurrent_writes`].
    ConcurrentWrites,
    /// Corresponds to [`StorageCapabilities::bulk_load`].
    BulkLoad,
    /// Corresponds to [`StorageCapabilities::sparql_endpoint`].
    SparqlEndpoint,
    /// Corresponds to [`StorageCapabilities::approximate_quad_count`].
    ApproximateQuadCount,
    /// Corresponds to [`StorageCapabilities::isolation`].
    Isolation(TransactionIsolation),
}

/// The default error type used by the in-tree adapters. Implementations are free
/// to use their own `Error` type as long as it is `Error + Send + Sync +
/// 'static`.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// An underlying I/O error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// A term/quad (de)serialization failure.
    #[error("serialization: {0}")]
    Serialization(String),
    /// Optimistic-concurrency-control conflict; callers treat this as retryable.
    #[error("transaction conflict")]
    TransactionConflict,
    /// A capability-gated operation was requested on a backend that does not
    /// advertise it.
    #[error("unsupported capability: {0:?}")]
    UnsupportedCapability(StorageCapability),
    /// An adapter-specific error that does not map to another variant.
    #[error("backend: {0}")]
    Backend(Box<dyn std::error::Error + Send + Sync>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_struct_is_copy_and_eq() {
        let a = StorageCapabilities {
            persistent: false,
            concurrent_writes: true,
            bulk_load: false,
            sparql_endpoint: true,
            approximate_quad_count: true,
            isolation: TransactionIsolation::SnapshotIsolation,
        };
        let b = a; // Copy
        assert_eq!(a, b);
    }

    #[test]
    fn storage_error_displays_unsupported_capability() {
        let e = StorageError::UnsupportedCapability(StorageCapability::BulkLoad);
        assert!(e.to_string().contains("unsupported capability"));
        assert!(e.to_string().contains("BulkLoad"));
    }

    #[test]
    fn storage_error_from_io() {
        let io = std::io::Error::other("boom");
        let e: StorageError = io.into();
        assert!(matches!(e, StorageError::Io(_)));
    }
}
