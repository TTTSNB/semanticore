# Changelog

All notable changes to the open-tier crates are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/). Before 1.0, minor versions may break APIs.

## [Unreleased]

### Added
- `semanticore-context` 0.1.0 (skeleton, RFC 0004): `Session` over enduring
  (file-backed Oxigraph), major and minor (in-memory Oxigraph) tiers, with
  `ContextItem`, pattern `ContextQuery`/`ContextQueryResult`, explicit and
  automatic promotion, eviction and demotion under `ScopeBounds`. SHACL and
  Datalog return a premium-only error; the OWL 2 EL reasoner, ANN index and
  SPARQL return a not-yet-implemented error.
- `semanticore-context-py` 0.1.0 (unpublished): PyO3 binding for
  `semanticore-context`, built with maturin as an abi3 wheel for CPython
  >= 3.10 and imported as `semanticore_context`. Exposes `Session` (with
  context-manager support, GIL released during calls), `SessionConfig`,
  `ContextItem`, `ContextQuery`, `ContextQueryResult` and their supporting
  types; every `SessionError` variant maps to its own Python exception class
  (including `PremiumOnlyError` and `NotYetImplementedError`). Ships a
  `.pyi` stub.
- `semanticore-store-oxigraph` 0.1.0: the first `Storage` backend, over
  Oxigraph 0.5.13, in-memory (`OxigraphStore::in_memory`) and file-backed
  (`OxigraphStore::open`, `OxigraphStore::open_read_only`).
- `semanticore-storage-testkit` (dev-only, unpublished): the RFC 0002 storage
  contract as functions generic over `S: Storage`, the
  `storage_contract_suite!` macro, quad strategies and the reference
  `MemStore`. `semanticore-core`'s storage tests now run through it, so every
  backend runs the identical suite.

## [0.1.0] - Unreleased

First release of `semanticore-core`: the backend-neutral storage and reasoner
trait surface specified in RFCs 0002 and 0003. It contains traits and result
types only; reasoner implementations live outside this crate.

### Added
- `storage`: the storage trait (RFC 0002).
- `reasoner`: the SHACL and Datalog reasoner traits, plus the OWL reasoner traits
  `classify`, `realize` and `check_consistency` (RFC 0003).
- `shacl`, `datalog`, `owl`: the result and error types for each reasoner family,
  including `OwlError` and a `License` error variant on `ShaclError` and
  `DatalogError`.
- `ClassHierarchy::unsatisfiable`, which lists the unsatisfiable classes found by
  `classify`.

### Changed
- SHACL validation results take `focus_node` as a `Term`, not an IRI only, so blank
  node and literal focus nodes can be reported.
- SHACL `conforms` is false when a result of any severity is present, not just a
  `Violation`.
- MSRV is 1.88, to match the workspace dependency graph.
