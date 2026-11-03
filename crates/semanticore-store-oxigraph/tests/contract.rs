//! The RFC 0002 storage contract, run against both Oxigraph variants with
//! the same suite `semanticore-core` runs against its reference `MemStore`.

use semanticore_storage_testkit::{storage_contract_suite, WithGuard};
use semanticore_store_oxigraph::OxigraphStore;
use tempfile::TempDir;

fn in_memory() -> OxigraphStore {
    OxigraphStore::in_memory().expect("in-memory store")
}

/// A file-backed store in a fresh temporary directory, deleted on drop.
fn on_disk() -> WithGuard<OxigraphStore, TempDir> {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = OxigraphStore::open(dir.path()).expect("open store");
    WithGuard::new(store, dir)
}

storage_contract_suite!(mod in_memory, cases = 256, fresh = in_memory);

// Each case opens a RocksDB directory, so fewer cases keep the run short.
storage_contract_suite!(mod on_disk, cases = 32, fresh = on_disk);
