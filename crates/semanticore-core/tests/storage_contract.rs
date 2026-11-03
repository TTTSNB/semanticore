//! The RFC 0002 `Storage` / `StorageTransaction` contract, run against the
//! test-only `MemStore`.
//!
//! The contract itself lives in the dev-only `semanticore-storage-testkit`
//! crate as functions generic over `S: Storage`; `storage_contract_suite!`
//! expands it into the tests below. Real backends (for example
//! `semanticore-store-oxigraph`) invoke the same macro, so every backend
//! runs the identical suite.

use semanticore_core::model::{NamedNode, Quad};
use semanticore_core::storage::{Storage, StorageError, StorageTransaction};
use semanticore_storage_testkit::{storage_contract_suite, MemStore};

storage_contract_suite!(mod mem_store, cases = 256, fresh = MemStore::new);

// ---------------------------------------------------------------------------
// MemStore-specific checks
// ---------------------------------------------------------------------------

/// `MemStore` uses optimistic concurrency control, so it can exercise the
/// retryable `TransactionConflict` error.
#[test]
fn conflicting_commits_report_transaction_conflict() {
    let store = MemStore::new();
    let q = Quad::triple(
        NamedNode::new("https://example.org/s").into(),
        NamedNode::new("https://example.org/p"),
        NamedNode::new("https://example.org/o").into(),
    );
    let mut a = store.begin_write().unwrap();
    let mut b = store.begin_write().unwrap();
    a.insert(&q).unwrap();
    b.insert(&q).unwrap();
    a.commit().unwrap();
    assert!(matches!(b.commit(), Err(StorageError::TransactionConflict)));
    assert_eq!(store.quad_count().unwrap(), 1);
    assert_eq!(store.snapshot().len(), 1);
}

#[test]
fn memstore_snapshot_of_empty_store_is_empty() {
    assert!(MemStore::new().snapshot().is_empty());
}
