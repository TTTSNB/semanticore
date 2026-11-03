//! [Oxigraph]-backed implementation of `semanticore-core`'s
//! [`Storage`] trait (RFC 0002).
//!
//! [`OxigraphStore`] comes in three variants:
//!
//! | Constructor | Backing | Use |
//! |---|---|---|
//! | [`OxigraphStore::in_memory`] | Oxigraph's in-memory store | working sets, tests |
//! | [`OxigraphStore::open`] | RocksDB directory, read-write | persistent (enduring) data |
//! | [`OxigraphStore::open_read_only`] | RocksDB directory, read-only | serving a store another process no longer writes |
//!
//! ```
//! use semanticore_core::model::{NamedNode, Quad};
//! use semanticore_core::storage::{Storage, StorageTransaction};
//! use semanticore_store_oxigraph::OxigraphStore;
//!
//! let store = OxigraphStore::in_memory()?;
//! let ex = |s: &str| NamedNode::new(format!("https://example.org/{s}"));
//! let quad = Quad::triple(ex("alice").into(), ex("knows"), ex("bob").into());
//!
//! let mut tx = store.begin_write()?;
//! assert!(tx.insert(&quad)?);
//! tx.commit()?;
//! assert_eq!(store.quad_count()?, 1);
//! # Ok::<(), semanticore_core::storage::StorageError>(())
//! ```
//!
//! # Behaviour
//!
//! - **Write transactions** wrap an Oxigraph transaction: reads see the
//!   transaction's own writes on top of the snapshot it started from;
//!   `commit` applies all writes atomically; `rollback` (or dropping the
//!   transaction) discards them.
//! - **In-memory writers are serialized.** Oxigraph's in-memory store holds a
//!   global lock for the life of a write transaction, so [`Storage::begin_write`]
//!   **blocks** while another write transaction is open, including one held
//!   by the same thread. Finish (commit, roll back or drop) one write
//!   transaction before starting the next on the same thread.
//! - **File-backed writers are not checked for conflicts.** Concurrent write
//!   transactions each read their own snapshot and commit as a batch; the
//!   last commit wins per quad. [`StorageError::TransactionConflict`] is
//!   never returned.
//! - **Read-only transactions** reject `insert` and `remove` with
//!   [`StorageError::Backend`] wrapping [`AdapterError::ReadOnlyTransaction`].
//!   On a read-write file-backed store they read one consistent snapshot. On
//!   the in-memory store, Oxigraph offers no public snapshot without taking
//!   the writer lock, so each read call sees the latest committed state
//!   (read committed); [`Storage::capabilities`] advertises this.
//! - **Named graphs**: [`Storage::named_graphs`] lists the graphs that hold
//!   at least one quad. Oxigraph keeps a graph name after its last quad is
//!   removed; such empty graphs are not listed.
//! - **Terms are stored verbatim or not at all.** The core model compares
//!   terms lexically, so a quad whose terms would not read back identical is
//!   refused on insert with [`StorageError::Serialization`]: IRIs that are
//!   not absolute, invalid blank-node labels or language tags, and XSD
//!   literals that Oxigraph would canonicalize (`"01"^^xsd:integer`,
//!   `"1"^^xsd:boolean`, any parsable `xsd:int`). Such a quad cannot be in
//!   the store, so `contains` returns `false`, `remove` returns `false` and a
//!   pattern using such a term matches nothing. Language tags keep their
//!   case.
//! - **Errors never panic.** Oxigraph I/O errors map to [`StorageError::Io`]
//!   (keeping the `io::ErrorKind`); corruption and other backend errors map
//!   to [`StorageError::Backend`] with the Oxigraph error as the source.
//!   Data written by other tools that the core model cannot carry (a graph
//!   named by a blank node, RDF 1.2 terms) is reported as
//!   [`StorageError::Serialization`] when read.
//!
//! [Oxigraph]: https://github.com/oxigraph/oxigraph

#![doc(html_root_url = "https://docs.rs/semanticore-store-oxigraph")]

mod convert;

use std::fmt;
use std::path::Path;

use oxigraph::store::{QuadIter, Store, Transaction};
use semanticore_core::model::{NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term};
use semanticore_core::storage::{
    Storage, StorageCapabilities, StorageError, StorageTransaction, TransactionIsolation,
};

use convert::{map_err, quad_from_ox, quad_to_ox};

/// Adapter-specific errors, carried inside [`StorageError::Backend`].
///
/// Recover one with `downcast_ref`:
///
/// ```
/// # use semanticore_core::model::{NamedNode, Quad};
/// # use semanticore_core::storage::{Storage, StorageError, StorageTransaction};
/// use semanticore_store_oxigraph::{AdapterError, OxigraphStore};
///
/// let store = OxigraphStore::in_memory()?;
/// # let n = NamedNode::new("https://example.org/n");
/// # let quad = Quad::triple(n.clone().into(), n.clone(), n.into());
/// let mut tx = store.begin_read()?;
/// match tx.insert(&quad) {
///     Err(StorageError::Backend(e)) => assert_eq!(
///         e.downcast_ref::<AdapterError>(),
///         Some(&AdapterError::ReadOnlyTransaction)
///     ),
///     other => panic!("unexpected {other:?}"),
/// }
/// # Ok::<(), StorageError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AdapterError {
    /// `insert` or `remove` was called on a transaction from
    /// [`Storage::begin_read`].
    ReadOnlyTransaction,
    /// [`Storage::begin_write`] was called on a store opened with
    /// [`OxigraphStore::open_read_only`].
    ReadOnlyStore,
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ReadOnlyTransaction => "transaction is read-only",
            Self::ReadOnlyStore => "store was opened read-only",
        })
    }
}

impl std::error::Error for AdapterError {}

impl From<AdapterError> for StorageError {
    fn from(e: AdapterError) -> Self {
        StorageError::Backend(Box::new(e))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    InMemory,
    OnDisk,
    OnDiskReadOnly,
}

/// An Oxigraph store behind the [`Storage`] trait. See the
/// [crate documentation](crate) for its transaction and term semantics.
pub struct OxigraphStore {
    inner: Store,
    kind: Kind,
}

impl fmt::Debug for OxigraphStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OxigraphStore")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl OxigraphStore {
    /// A new, empty in-memory store. Not persistent.
    pub fn in_memory() -> Result<Self, StorageError> {
        Ok(Self {
            inner: Store::new().map_err(map_err)?,
            kind: Kind::InMemory,
        })
    }

    /// Open (or create) a persistent store in the directory `path`.
    ///
    /// Only one process may have a directory open read-write at a time; a
    /// second open fails with an error rather than blocking.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        Ok(Self {
            inner: Store::open(path).map_err(map_err)?,
            kind: Kind::OnDisk,
        })
    }

    /// Open an existing persistent store read-only.
    ///
    /// [`Storage::begin_write`] then fails with [`AdapterError::ReadOnlyStore`].
    /// Oxigraph does not support reading a directory while another process
    /// writes to it; open read-only only once writers have finished.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        Ok(Self {
            inner: Store::open_read_only(path).map_err(map_err)?,
            kind: Kind::OnDiskReadOnly,
        })
    }

    /// The underlying Oxigraph store, for what the [`Storage`] trait does not
    /// cover: SPARQL queries (`sparql_endpoint` capability), the bulk loader
    /// (`bulk_load` capability), flushing and backups.
    ///
    /// Writes made through it bypass this adapter's term checks; data the
    /// core model cannot carry then surfaces as
    /// [`StorageError::Serialization`] on read.
    pub fn oxigraph(&self) -> &Store {
        &self.inner
    }
}

impl Storage for OxigraphStore {
    type Transaction<'a> = OxigraphTransaction<'a>;
    type Error = StorageError;

    fn begin_read(&self) -> Result<OxigraphTransaction<'_>, StorageError> {
        let inner = match self.kind {
            // A RocksDB transaction is a snapshot plus a private write batch;
            // it takes no lock, so a never-committed one is a cheap
            // consistent read view.
            Kind::OnDisk => TxInner::Snapshot(self.inner.start_transaction().map_err(map_err)?),
            // In memory, starting a transaction takes the global writer lock.
            // A read-only RocksDB instance refuses transactions, but its data
            // cannot change underneath us anyway.
            Kind::InMemory | Kind::OnDiskReadOnly => TxInner::Latest(&self.inner),
        };
        Ok(OxigraphTransaction { inner })
    }

    fn begin_write(&self) -> Result<OxigraphTransaction<'_>, StorageError> {
        if self.kind == Kind::OnDiskReadOnly {
            return Err(AdapterError::ReadOnlyStore.into());
        }
        Ok(OxigraphTransaction {
            inner: TxInner::Write(self.inner.start_transaction().map_err(map_err)?),
        })
    }

    /// Exact, by a full scan (Oxigraph keeps no counter); see
    /// [`StorageCapabilities::approximate_quad_count`].
    fn quad_count(&self) -> Result<u64, StorageError> {
        Ok(self.inner.len().map_err(map_err)? as u64)
    }

    fn named_graphs(&self) -> Result<Vec<NamedNode>, StorageError> {
        let mut graphs = Vec::new();
        for g in self.inner.named_graphs() {
            let g = match g.map_err(map_err)? {
                oxigraph::model::NamedOrBlankNode::NamedNode(n) => n,
                oxigraph::model::NamedOrBlankNode::BlankNode(b) => {
                    return Err(
                        convert::Unrepresentable::BlankGraphName(b.as_str().to_owned()).into(),
                    )
                }
            };
            // Oxigraph remembers graph names whose quads were all removed.
            let first = self
                .inner
                .quads_for_pattern(None, None, None, Some(g.as_ref().into()))
                .next();
            if let Some(first) = first {
                first.map_err(map_err)?;
                graphs.push(convert::named_from_ox(&g));
            }
        }
        Ok(graphs)
    }

    fn capabilities(&self) -> StorageCapabilities {
        match self.kind {
            Kind::InMemory => StorageCapabilities {
                persistent: false,
                concurrent_writes: true,
                bulk_load: false,
                sparql_endpoint: true,
                approximate_quad_count: false,
                isolation: TransactionIsolation::ReadCommitted,
            },
            Kind::OnDisk => StorageCapabilities {
                persistent: true,
                concurrent_writes: true,
                bulk_load: true,
                sparql_endpoint: true,
                approximate_quad_count: false,
                isolation: TransactionIsolation::SnapshotIsolation,
            },
            Kind::OnDiskReadOnly => StorageCapabilities {
                persistent: true,
                concurrent_writes: false,
                bulk_load: false,
                sparql_endpoint: true,
                approximate_quad_count: false,
                isolation: TransactionIsolation::SnapshotIsolation,
            },
        }
    }
}

/// A transaction on an [`OxigraphStore`].
pub struct OxigraphTransaction<'a> {
    inner: TxInner<'a>,
}

enum TxInner<'a> {
    /// Read-write.
    Write(Transaction<'a>),
    /// Read-only over one snapshot; dropped, never committed.
    Snapshot(Transaction<'a>),
    /// Read-only; each call reads the latest committed state.
    Latest(&'a Store),
}

impl fmt::Debug for OxigraphTransaction<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = match self.inner {
            TxInner::Write(_) => "write",
            TxInner::Snapshot(_) => "read (snapshot)",
            TxInner::Latest(_) => "read (latest)",
        };
        f.debug_struct("OxigraphTransaction")
            .field("mode", &mode)
            .finish()
    }
}

impl<'a> OxigraphTransaction<'a> {
    fn writer(&mut self) -> Result<&mut Transaction<'a>, StorageError> {
        match &mut self.inner {
            TxInner::Write(tx) => Ok(tx),
            TxInner::Snapshot(_) | TxInner::Latest(_) => {
                Err(AdapterError::ReadOnlyTransaction.into())
            }
        }
    }

    fn contains_ox(&self, quad: &oxigraph::model::Quad) -> Result<bool, StorageError> {
        match &self.inner {
            TxInner::Write(tx) | TxInner::Snapshot(tx) => tx.contains(quad),
            TxInner::Latest(store) => store.contains(quad),
        }
        .map_err(map_err)
    }
}

impl StorageTransaction for OxigraphTransaction<'_> {
    type Error = StorageError;

    fn insert(&mut self, quad: &Quad) -> Result<bool, StorageError> {
        self.writer()?;
        let quad = quad_to_ox(quad)?;
        if self.contains_ox(&quad)? {
            return Ok(false);
        }
        self.writer()?.insert(&quad);
        Ok(true)
    }

    fn remove(&mut self, quad: &Quad) -> Result<bool, StorageError> {
        self.writer()?;
        let Ok(quad) = quad_to_ox(quad) else {
            return Ok(false);
        };
        if !self.contains_ox(&quad)? {
            return Ok(false);
        }
        self.writer()?.remove(&quad);
        Ok(true)
    }

    fn contains(&self, quad: &Quad) -> Result<bool, StorageError> {
        match quad_to_ox(quad) {
            Ok(quad) => self.contains_ox(&quad),
            Err(_) => Ok(false),
        }
    }

    fn quads_for_pattern<'a>(
        &'a self,
        s: Option<&NamedOrBlankNode>,
        p: Option<&NamedNode>,
        o: Option<&Term>,
        g: Option<&NamedOrDefault>,
    ) -> Box<dyn Iterator<Item = Result<Quad, StorageError>> + 'a> {
        // A bound term the store cannot hold matches nothing.
        let (Ok(s), Ok(p), Ok(o), Ok(g)) = (
            s.map(convert::subject_to_ox).transpose(),
            p.map(convert::named_to_ox).transpose(),
            o.map(convert::term_to_ox).transpose(),
            g.map(convert::graph_to_ox).transpose(),
        ) else {
            return Box::new(std::iter::empty());
        };
        let (s, p, o, g) = (
            s.as_ref().map(Into::into),
            p.as_ref().map(Into::into),
            o.as_ref().map(Into::into),
            g.as_ref().map(Into::into),
        );
        let iter: QuadIter<'a> = match &self.inner {
            TxInner::Write(tx) | TxInner::Snapshot(tx) => tx.quads_for_pattern(s, p, o, g),
            TxInner::Latest(store) => store.quads_for_pattern(s, p, o, g),
        };
        Box::new(iter.map(|r| r.map_err(map_err).and_then(|q| quad_from_ox(&q))))
    }

    fn commit(self) -> Result<(), StorageError> {
        match self.inner {
            TxInner::Write(tx) => tx.commit().map_err(map_err),
            TxInner::Snapshot(_) | TxInner::Latest(_) => Ok(()),
        }
    }

    fn rollback(self) -> Result<(), StorageError> {
        // Dropping an Oxigraph transaction discards its writes and releases
        // the in-memory writer lock.
        Ok(())
    }
}
