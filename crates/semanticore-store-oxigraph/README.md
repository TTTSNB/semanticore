# semanticore-store-oxigraph

An [Oxigraph](https://github.com/oxigraph/oxigraph)-backed implementation of the
`Storage` trait from [`semanticore-core`](../semanticore-core) (RFC 0002).

```rust
use semanticore_core::model::{NamedNode, Quad};
use semanticore_core::storage::{Storage, StorageTransaction};
use semanticore_store_oxigraph::OxigraphStore;

let store = OxigraphStore::open("data/enduring")?; // or OxigraphStore::in_memory()?
let ex = |s: &str| NamedNode::new(format!("https://example.org/{s}"));
let mut tx = store.begin_write()?;
tx.insert(&Quad::triple(ex("alice").into(), ex("knows"), ex("bob").into()))?;
tx.commit()?;
# Ok::<(), semanticore_core::storage::StorageError>(())
```

## Variants and capabilities

| | `in_memory()` | `open(path)` | `open_read_only(path)` |
|---|---|---|---|
| `persistent` | no | yes | yes |
| `concurrent_writes` | yes (writers wait for a global lock) | yes (no conflict detection) | no |
| `bulk_load` | no | yes, via `oxigraph().bulk_loader()` | no |
| `sparql_endpoint` | yes, via `oxigraph()` | yes | yes |
| `approximate_quad_count` | no (exact, full scan) | no (exact, full scan) | no |
| `isolation` of read-only transactions | read committed | snapshot | snapshot (data is static) |

## Things to know

- **In-memory write transactions block each other**, including on the same
  thread: finish one before starting the next.
- **File-backed writers are not conflict-checked**; the last commit wins.
  `TransactionConflict` is never returned.
- **Terms are stored verbatim or refused.** A quad that would not read back
  identical (an IRI that is not absolute, an invalid blank-node label or
  language tag, or an XSD literal Oxigraph would canonicalize, such as
  `"01"^^xsd:integer`) is rejected with `StorageError::Serialization`.
  Language tags keep their case.
- **Errors never panic.** Oxigraph I/O errors become `StorageError::Io`;
  everything else becomes `StorageError::Backend` with the Oxigraph error as its
  source. Read-only violations carry `AdapterError` (downcast it from `Backend`).

The full contract tests are shared with every other backend through the
dev-only `semanticore-storage-testkit` crate.

## License

Apache-2.0. Oxigraph is MIT OR Apache-2.0; RocksDB (via `oxrocksdb-sys`) is
GPL-2.0 OR Apache-2.0, used here under Apache-2.0.
