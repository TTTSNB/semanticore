//! A reference in-memory `Storage` implementation. Test double only.
//!
//! `MemStore` gives the contract a second, deliberately simple
//! implementation to run against, so a failure in a real backend can be told
//! apart from a mistake in the contract itself.

use std::collections::HashSet;
use std::sync::Mutex;

use semanticore_core::model::{NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term};
use semanticore_core::storage::{
    Storage, StorageCapabilities, StorageError, StorageTransaction, TransactionIsolation,
};

use crate::matches_pattern;

#[derive(Default)]
struct State {
    /// Bumped on every successful write commit; used for optimistic
    /// concurrency control so `TransactionConflict` is exercised.
    version: u64,
    quads: HashSet<Quad>,
}

/// A minimal snapshot-isolated, in-memory quad store.
#[derive(Default)]
pub struct MemStore {
    state: Mutex<State>,
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convenience: insert `quads` in one committed write transaction.
    pub fn with_quads(quads: &[Quad]) -> Self {
        let store = Self::new();
        crate::contract::load(&store, quads);
        store
    }

    /// The committed state, for assertions.
    pub fn snapshot(&self) -> HashSet<Quad> {
        self.state.lock().unwrap().quads.clone()
    }
}

/// A transaction over a private copy of the committed state.
pub struct MemTx<'a> {
    store: &'a MemStore,
    base_version: u64,
    working: HashSet<Quad>,
    writable: bool,
}

fn read_only_error() -> StorageError {
    StorageError::Backend("write attempted in a read-only transaction".into())
}

impl Storage for MemStore {
    type Transaction<'a> = MemTx<'a>;
    type Error = StorageError;

    fn begin_read(&self) -> Result<MemTx<'_>, StorageError> {
        let st = self.state.lock().unwrap();
        Ok(MemTx {
            store: self,
            base_version: st.version,
            working: st.quads.clone(),
            writable: false,
        })
    }

    fn begin_write(&self) -> Result<MemTx<'_>, StorageError> {
        let mut tx = self.begin_read()?;
        tx.writable = true;
        Ok(tx)
    }

    fn quad_count(&self) -> Result<u64, StorageError> {
        Ok(self.state.lock().unwrap().quads.len() as u64)
    }

    fn named_graphs(&self) -> Result<Vec<NamedNode>, StorageError> {
        let st = self.state.lock().unwrap();
        let mut graphs: Vec<NamedNode> = st
            .quads
            .iter()
            .filter_map(|q| match &q.graph {
                NamedOrDefault::Named(g) => Some(g.clone()),
                NamedOrDefault::Default => None,
            })
            .collect();
        graphs.sort();
        graphs.dedup();
        Ok(graphs)
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities {
            persistent: false,
            concurrent_writes: false,
            bulk_load: false,
            sparql_endpoint: false,
            approximate_quad_count: false,
            isolation: TransactionIsolation::SnapshotIsolation,
        }
    }
}

impl StorageTransaction for MemTx<'_> {
    type Error = StorageError;

    fn insert(&mut self, quad: &Quad) -> Result<bool, StorageError> {
        if !self.writable {
            return Err(read_only_error());
        }
        Ok(self.working.insert(quad.clone()))
    }

    fn remove(&mut self, quad: &Quad) -> Result<bool, StorageError> {
        if !self.writable {
            return Err(read_only_error());
        }
        Ok(self.working.remove(quad))
    }

    fn contains(&self, quad: &Quad) -> Result<bool, StorageError> {
        Ok(self.working.contains(quad))
    }

    fn quads_for_pattern<'a>(
        &'a self,
        s: Option<&NamedOrBlankNode>,
        p: Option<&NamedNode>,
        o: Option<&Term>,
        g: Option<&NamedOrDefault>,
    ) -> Box<dyn Iterator<Item = Result<Quad, StorageError>> + 'a> {
        let (s, p, o, g) = (s.cloned(), p.cloned(), o.cloned(), g.cloned());
        Box::new(
            self.working
                .iter()
                .filter(move |q| matches_pattern(q, s.as_ref(), p.as_ref(), o.as_ref(), g.as_ref()))
                .cloned()
                .map(Ok),
        )
    }

    fn commit(self) -> Result<(), StorageError> {
        if !self.writable {
            return Ok(());
        }
        let mut st = self.store.state.lock().unwrap();
        if st.version != self.base_version {
            return Err(StorageError::TransactionConflict);
        }
        st.quads = self.working;
        st.version += 1;
        Ok(())
    }

    fn rollback(self) -> Result<(), StorageError> {
        Ok(())
    }
}
