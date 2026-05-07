# RFC 0002: Storage trait and Oxigraph backend

- **Status:** Accepted
- **Date:** 2026-05-07
- **Author:** Elliott Risch
- **Phase:** 2 — Architecture spec
- **Depends on:** RFC 0001
- **Crates affected:** `semanticore-core` (defines), all reasoners + context engines (consume)

## Summary

This RFC specifies the `Storage` and `StorageTransaction` traits that live in `semanticore-core` and are consumed by every reasoner, every Context engine, and every higher-level surface in the workspace. It pins the sync-default / async-on-demand strategy, fixes the error model around an associated `Error` type with a default `StorageError` enum, defines a `StorageCapabilities` advertisement so reasoners can refuse to run on an under-equipped backend, and sketches the Oxigraph adapter that ships as the in-process default. Adapters for GraphDB, Neptune, and Blazegraph are described as deferred future work; nothing beyond Oxigraph ships in v0.1. Every subsequent RFC that touches reasoning, Context surfaces, or projection bottoms out on these trait signatures, so this document is the second load-bearing piece of the Phase 2 specification after RFC 0001.

## Motivation

The previous Python-first SDK at the legacy `~/Desktop/development/semantic-core/semanticore` repo had no Storage abstraction. Triples lived directly inside the reasoner's working memory, the projection layer talked to its own ad-hoc graph wrapper, and any swap of underlying store required surgery in three places. That coupling is the single largest source of correctness drift we observed across the Theseus, Metasemantics, and Nucleus prototypes: a bug fixed in one store implementation invariably resurfaced in the others, because none of them shared a contract.

A trait-based abstraction solves this. Every reasoner takes `&impl Storage` (or `&dyn Storage` when boxed), every Context engine treats the store as an opaque transactional graph, and the choice of backend becomes a runtime decision rather than a compile-time entanglement. Pluggability is not optional — the commercial story for semantiCore explicitly assumes that enterprise users will plug in GraphDB, Neptune, Blazegraph, or RDFox-fronted stores rather than ship their data into Oxigraph. The OSS tier that lives in this repo only needs to ship one production-quality backend (Oxigraph) and the trait everyone else extends.

A second motivation is reasoning-correctness isolation. We do not want any reasoner's correctness proof to depend on a specific store's quirks (lexical-form normalization, blank-node skolemization timing, default-graph semantics). The `Storage` trait pins exactly the operations a reasoner is allowed to perform; any backend that satisfies the trait satisfies the reasoner. Capability flags handle the cases where a reasoner needs more than the minimum (snapshot isolation, bulk-load) — the reasoner asks the store what it can do, and refuses to run if the answer is no.

Oxigraph is the embedded default for three reasons. It is a pure-Rust SPARQL 1.1-compliant store with a permissive license (Apache 2.0 / MIT), it ships with both in-memory and on-disk backends out of the same crate, and its transactional API is already shaped almost identically to what we want our trait to expose. Adopting Oxigraph as the default is a decision to lean on a well-maintained dependency for the part of the stack we have no business reinventing in v0.1.

## Trait surface

### `Storage`

The top-level trait. Every operation that requires reading or writing quads goes through a transaction; the `Storage` trait itself only exposes transaction creation, schema-level metadata, and capability advertisement.

```rust
pub trait Storage: Send + Sync {
    type Transaction<'a>: StorageTransaction where Self: 'a;
    type Error: std::error::Error + Send + Sync + 'static;

    fn begin_read(&self) -> Result<Self::Transaction<'_>, Self::Error>;
    fn begin_write(&self) -> Result<Self::Transaction<'_>, Self::Error>;

    fn quad_count(&self) -> Result<u64, Self::Error>;
    fn named_graphs(&self) -> Result<Vec<NamedNode>, Self::Error>;

    fn capabilities(&self) -> StorageCapabilities;
}
```

`Send + Sync` is required because every reasoner and Context engine assumes the store is shareable across threads — the OSS default reasoners are single-process but multi-threaded, and the premium distributed engines are multi-process. Stores that are not internally `Sync` must wrap themselves in an interior-mutability primitive (e.g. `parking_lot::Mutex`) inside the adapter.

The associated `Transaction<'a>` type uses generic associated types (GATs, stable since Rust 1.65) so transactions can borrow from the store without forcing every implementation into `Arc`-based reference counting. The lifetime parameter ties the transaction to the store; commit/rollback consume the transaction and release the borrow.

`quad_count` and `named_graphs` are exposed at the store level (not the transaction level) because they describe the store's overall state, not a per-transaction snapshot. Implementations that can only answer these accurately within a transaction (rare) may begin a short-lived read transaction internally.

### `StorageTransaction`

Per-transaction operations. Read-only transactions (from `begin_read`) must reject `insert` and `remove` with `Self::Error` rather than panicking — the type system does not split read and write transactions because that doubles the API surface for negligible safety gain in practice.

```rust
pub trait StorageTransaction {
    type Error;

    fn insert(&mut self, quad: &Quad) -> Result<bool, Self::Error>;
    fn remove(&mut self, quad: &Quad) -> Result<bool, Self::Error>;

    fn contains(&self, quad: &Quad) -> Result<bool, Self::Error>;

    fn quads_for_pattern<'a>(
        &'a self,
        s: Option<&NamedOrBlankNode>,
        p: Option<&NamedNode>,
        o: Option<&Term>,
        g: Option<&NamedOrDefault>,
    ) -> Box<dyn Iterator<Item = Result<Quad, Self::Error>> + 'a>;

    fn commit(self) -> Result<(), Self::Error>;
    fn rollback(self) -> Result<(), Self::Error>;
}
```

`insert` and `remove` return `bool` to indicate whether the store's state actually changed (the quad was newly inserted, or it was actually present and got removed). This matches Oxigraph's semantics and is information that several reasoners (notably the saturation-based OWL RL impl in T4.3) rely on for incremental fixpoint detection.

`quads_for_pattern` returns a boxed iterator rather than a concrete type because GATs-on-iterators are still rough in stable Rust as of 1.78, and the dyn dispatch cost is dominated by I/O in any realistic backend. The four position arguments use the standard RDF term hierarchy: subject is `NamedOrBlankNode`, predicate is `NamedNode`, object is `Term` (which subsumes literals, blanks, and named nodes), and graph is `NamedOrDefault` to cover both named-graph and default-graph quads. `None` for any position means "match anything in that position."

`commit` and `rollback` consume `self`; this is the only way to guarantee at the type level that a transaction cannot be used after it terminates. Implementations that need to flush write buffers (RocksDB-backed Oxigraph, on-disk variants) do their flushing inside `commit`; rollback is expected to be cheap and infallible in practice but the result type is preserved for adapters that genuinely can fail to roll back (e.g. an HTTP backend that loses its connection mid-transaction).

### `StorageCapabilities`

Capabilities are how a backend advertises what it supports. Reasoners check capabilities before running and refuse with `StorageError::UnsupportedCapability` if a hard requirement is not met.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageCapabilities {
    pub persistent: bool,
    pub concurrent_writes: bool,
    pub bulk_load: bool,
    pub sparql_endpoint: bool,
    pub approximate_quad_count: bool,
    pub isolation: TransactionIsolation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionIsolation {
    ReadCommitted,
    SnapshotIsolation,
    Serializable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageCapability {
    Persistent,
    ConcurrentWrites,
    BulkLoad,
    SparqlEndpoint,
    ApproximateQuadCount,
    Isolation(TransactionIsolation),
}
```

The struct form is used for advertisement (`capabilities()` returns one); the enum form is used for capability requests in error variants and reasoner declarations (e.g. "the OWL DL reasoner requires `Isolation(SnapshotIsolation)` or stronger"). The two stay in sync by convention, not by macro — there is one struct field per capability variant, deliberately enumerated.

`approximate_quad_count` distinguishes stores where `quad_count` is O(1) (Oxigraph in-memory keeps a counter) from stores where it is O(n) or unavailable; reasoners that branch on store size use the capability to decide whether to call `quad_count` at all.

`sparql_endpoint` exists because some backends can execute full SPARQL queries internally (Oxigraph can; a hypothetical hash-set-of-triples adapter cannot). The Context query layer (RFC 0004) checks this capability before delegating SPARQL execution to the store; if the capability is missing, the query layer falls back to its own pattern-evaluator on top of `quads_for_pattern`.

## Sync vs. async strategy

Default: synchronous trait. Async wrappers (e.g. for HTTP-backed remote stores like GraphDB Workbench or Neptune Gremlin) implement a parallel `AsyncStorage` trait with the same shape but `async fn` methods.

The justification is empirical. The bulk of in-process reasoning is CPU-bound: forward-chaining saturation, structural subsumption, sound-and-complete materialization. None of those operations gain anything from async — the executor would just shuttle the same thread between sync work units. Async also forces `Pin<Box<dyn Future>>` plumbing through every reasoner trait, which costs both ergonomics and compile time.

Remote backends are different. An HTTP round trip to a GraphDB Workbench endpoint is dominated by network latency, and a reasoner that batches a thousand pattern-matches over HTTP will benefit measurably from concurrent in-flight requests. For those backends we ship a parallel `AsyncStorage` trait whose shape mirrors `Storage` exactly:

```rust
pub trait AsyncStorage: Send + Sync {
    type Transaction<'a>: AsyncStorageTransaction where Self: 'a;
    type Error: std::error::Error + Send + Sync + 'static;

    async fn begin_read(&self) -> Result<Self::Transaction<'_>, Self::Error>;
    async fn begin_write(&self) -> Result<Self::Transaction<'_>, Self::Error>;
    async fn quad_count(&self) -> Result<u64, Self::Error>;
    async fn named_graphs(&self) -> Result<Vec<NamedNode>, Self::Error>;
    fn capabilities(&self) -> StorageCapabilities;
}
```

Any in-process `Storage` impl can be wrapped in a `BlockingAsyncStorage` adapter (uses `tokio::task::spawn_blocking`) to satisfy code paths that need an `AsyncStorage`. The reverse is not true: an `AsyncStorage` cannot be safely run from a sync context without a runtime, so reasoners that take an `AsyncStorage` are explicitly opt-in.

For v0.1, only the synchronous trait ships. `AsyncStorage` lands when the first remote backend lands — currently slated for Phase 5 alongside benchmarks against GraphDB.

## Error model

Implementations define their own error types via the associated `Error` type, but every error type must implement `std::error::Error + Send + Sync + 'static`. This lets callers either match concretely (when they know the implementation) or box up to `Box<dyn Error>` (when they are generic).

The crate provides a default `StorageError` enum that the Oxigraph adapter and the in-memory adapter both use:

```rust
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization: {0}")]
    Serialization(String),
    #[error("transaction conflict")]
    TransactionConflict,
    #[error("unsupported capability: {0:?}")]
    UnsupportedCapability(StorageCapability),
    #[error("backend: {0}")]
    Backend(Box<dyn std::error::Error + Send + Sync>),
}
```

`Backend` is the escape hatch for adapter-specific errors that do not map cleanly to the other variants — Oxigraph's storage errors get wrapped through `Backend` rather than each one getting its own variant. `TransactionConflict` is the canonical optimistic-concurrency-control failure; reasoners and Context engines treat it as retryable, all other variants as fatal-to-the-current-operation.

`UnsupportedCapability` is what a backend returns when a reasoner asks for an operation outside its capability set (e.g. calling a bulk-load API on a store that does not advertise `bulk_load`). The reasoner is expected to have checked capabilities before issuing the call; if it issues anyway, this is the error it gets.

## Oxigraph adapter sketch

Oxigraph's `oxigraph::store::Store` exposes `start_transaction`, `quads_for_pattern`, `insert`, `remove`, and `commit` directly, so the adapter is mostly a one-to-one shim. The two non-trivial pieces are (1) translating between Oxigraph's term types (`oxigraph::model::Quad`) and our own (`semanticore_core::model::Quad`) — these will be near-isomorphic but not identical, since semanticore-core wants to pin a few quirks Oxigraph leaves implementation-defined (lexical-form normalization on numeric literals, default-graph IRI choice) — and (2) mapping Oxigraph's storage errors into our `StorageError`.

Capability advertisement for the in-memory variant: `persistent=false`, `concurrent_writes=true` (Oxigraph uses a global write lock internally), `bulk_load=false`, `sparql_endpoint=true`, `approximate_quad_count=true`, `isolation=SnapshotIsolation`. The on-disk (`oxigraph::store::Store::open`) variant flips `persistent=true` and `bulk_load=true` (Oxigraph exposes a `bulk_loader` API for fast initial ingest).

A 30-line sketch of the adapter:

```rust
use oxigraph::store::Store as OxStore;
use semanticore_core::storage::{
    Storage, StorageCapabilities, StorageError, TransactionIsolation,
};

pub struct OxigraphAdapter {
    inner: OxStore,
    persistent: bool,
}

impl Storage for OxigraphAdapter {
    type Transaction<'a> = OxigraphTx<'a> where Self: 'a;
    type Error = StorageError;

    fn begin_read(&self) -> Result<Self::Transaction<'_>, Self::Error> {
        Ok(OxigraphTx::Read(self.inner.snapshot()))
    }

    fn begin_write(&self) -> Result<Self::Transaction<'_>, Self::Error> {
        Ok(OxigraphTx::Write(self.inner.start_transaction()
            .map_err(|e| StorageError::Backend(Box::new(e)))?))
    }

    fn quad_count(&self) -> Result<u64, Self::Error> {
        Ok(self.inner.len()
            .map_err(|e| StorageError::Backend(Box::new(e)))? as u64)
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities {
            persistent: self.persistent,
            concurrent_writes: true,
            bulk_load: self.persistent,
            sparql_endpoint: true,
            approximate_quad_count: true,
            isolation: TransactionIsolation::SnapshotIsolation,
        }
    }
    // named_graphs elided for brevity
}
```

`OxigraphTx` is an enum with `Read(Snapshot)` and `Write(Transaction)` variants; the `StorageTransaction` impl matches on the variant and either delegates to Oxigraph's snapshot read API or its transaction write API. Read transactions reject `insert` and `remove` with `StorageError::Backend(...)` carrying a sentinel "transaction is read-only" inner error.

## Adapter sketches: GraphDB, Neptune, Blazegraph

**GraphDB (Ontotext).** HTTP via the GraphDB Workbench REST API. SPARQL endpoint capability is supported natively; transactions are exposed via the `/repositories/{id}/transactions` endpoint. Implementation is async-only — every operation is a network round trip — and slots into `AsyncStorage` rather than `Storage`. Capabilities: `persistent=true`, `concurrent_writes=true`, `sparql_endpoint=true`, `bulk_load=true` (via the bulk-import endpoint). Not implemented in v0.1; planned for Phase 5 alongside the benchmark suite.

**AWS Neptune.** Two interfaces: SPARQL HTTP for RDF, Gremlin HTTP for property graph. The `Storage` adapter targets the SPARQL interface (RDF is the canonical representation in semantiCore-core); the property-graph interface is consumed by `semanticore-projection` via a separate path, not via `Storage`. Async-only. Capabilities: `persistent=true`, `concurrent_writes=true`, `sparql_endpoint=true`, `bulk_load=true` (via Neptune's S3 bulk-load API), `isolation=ReadCommitted` (Neptune's documented default). Not implemented in v0.1.

**Blazegraph.** HTTP, supports SPARQL plus reification-done-right (RDR) extensions. Single-machine but well-instrumented; its main role in this RFC is as a benchmark target, not a production adapter. Async-only. Capabilities: `persistent=true`, `concurrent_writes=false` (Blazegraph serializes writes), `sparql_endpoint=true`. Not implemented in v0.1.

None of the three are required for v0.1. They appear here because the trait must be shaped such that they can land later without breaking changes — the `AsyncStorage` parallel trait, the capability enum, and the boxed-iterator pattern in `quads_for_pattern` are the three concessions to that future requirement.

## Performance contract

These are the contract terms every `Storage` impl must meet, called out so reasoner implementations can rely on them.

- Insertion is amortized O(1) for in-memory backends and O(log n) for on-disk backends; insertion that triggers an internal compaction or rebalancing remains O(log n) amortized.
- Pattern iteration must be lazy. `quads_for_pattern` returns an `Iterator`, not a `Vec`, and reasoners are expected to stream — backends that materialize internally will run out of memory on graphs of any practical size.
- `quad_count` may be O(1) approximate or O(n) exact. Backends declare which via the `approximate_quad_count` capability flag. Reasoners that need an exact count and only have an approximate-count backend must walk the iterator themselves.
- Transactions support read-during-write inside the same transaction (read-your-writes consistency). A reasoner that inserts a quad and then issues `quads_for_pattern` matching that quad in the same transaction must see the inserted quad in the iterator. Implementations that cannot meet this contract directly (e.g. a write-buffered HTTP backend) must implement a local read-set inside the transaction object.
- `commit` is allowed to be slow (flushes, fsyncs, network round trips); `rollback` is expected to be fast and is called on hot paths in reasoners that speculatively saturate.

## Open questions

- **Quad vs. triple in default API.** Resolved: use Quad universally, with default-graph quads carrying the default-graph IRI in the graph slot. Triple-only stores wrap themselves and reject any non-default-graph quad with `StorageError::Backend`. The split-trait alternative ("offer `TripleStore` as a base, `QuadStore: TripleStore` as an extension") was considered and rejected — it doubles the trait surface for a distinction that vanishes in every realistic backend.
- **Bulk-load API for ingest workflows.** Deferred. The `bulk_load=true` capability is advertised by Oxigraph's on-disk variant, but the actual bulk-load entry point is not part of the `Storage` trait in v0.1. Instead, callers reach for a backend-specific `bulk_loader` method on the concrete adapter type. Phase 3 may add a `BulkLoad` extension trait if the ergonomics demand it.
- **Federation.** Multiple `Storage` instances composed across machines (a federated SPARQL query plane) is a premium-tier concern and lives in `semanticore-context-engine-major`. Not in v0.1; tracked in RFC 0004 (Context API) for the federation hooks `semanticore-context` will need.
- **SPARQL endpoint as a `Storage` capability vs. a separate trait.** Resolved: capability flag on `Storage`, not a separate trait. The SPARQL execution surface (parser, planner, results bindings) lives in the Context query layer per RFC 0004; `Storage` only advertises whether the backend itself can absorb a SPARQL query string and return results, which it then exposes via a method that lives outside this RFC's trait surface.

## Migration notes

The legacy SDK at `~/Desktop/development/semantic-core/semanticore` had no Storage abstraction — triples lived inside the reasoner's working memory, the projection layer used its own ad-hoc graph wrapper, and any swap of underlying store required surgery in three places. This RFC is the first place a single `Storage` trait exists across the entire workspace, and once `semanticore-core` ships in T3.1, it becomes the only place any new code is allowed to talk to a triple/quad store. Older code paths in the legacy repo are out of scope for v0.1: per the Phase 0 resolution to do a clean break, no compatibility shim will exist between the legacy storage layer and this trait. Consumers migrate by rewriting against the trait.
