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
