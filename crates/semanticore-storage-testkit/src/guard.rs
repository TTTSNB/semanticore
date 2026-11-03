//! [`WithGuard`]: a store that keeps something else alive alongside it.

use semanticore_core::model::NamedNode;
use semanticore_core::storage::{Storage, StorageCapabilities};

/// A store bundled with a guard that must outlive it, such as the temporary
/// directory a file-backed store lives in. Every `Storage` method delegates
/// to `store`.
///
/// Fields drop in declaration order, so the store is closed before the guard
/// is released.
pub struct WithGuard<S, G> {
    store: S,
    _guard: G,
}

impl<S, G> WithGuard<S, G> {
    pub fn new(store: S, guard: G) -> Self {
        Self {
            store,
            _guard: guard,
        }
    }

    pub fn store(&self) -> &S {
        &self.store
    }
}

impl<S: Storage, G: Send + Sync> Storage for WithGuard<S, G> {
    type Transaction<'a>
        = S::Transaction<'a>
    where
        Self: 'a;
    type Error = S::Error;

    fn begin_read(&self) -> Result<Self::Transaction<'_>, Self::Error> {
        self.store.begin_read()
    }

    fn begin_write(&self) -> Result<Self::Transaction<'_>, Self::Error> {
        self.store.begin_write()
    }

    fn quad_count(&self) -> Result<u64, Self::Error> {
        self.store.quad_count()
    }

    fn named_graphs(&self) -> Result<Vec<NamedNode>, Self::Error> {
        self.store.named_graphs()
    }

    fn capabilities(&self) -> StorageCapabilities {
        self.store.capabilities()
    }
}
