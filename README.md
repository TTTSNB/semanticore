# semantiCore

A Rust-core, two-tier semantic platform for production knowledge graphs.

semantiCore is a Rust-core project with a two-tier distribution: an open core under Apache 2.0, and a premium tier of distributed reasoners and engines available under a commercial license. Enterprise Knowledge serves as the commercial steward, mirroring the pattern set by Equinor's maplib and Treehouse: open primitives stay open, while production-scale infrastructure ships as a supported product.

## Architecture

The full architecture is documented across six RFCs:

- [RFC 0001](docs/rfcs/0001-crate-graph.md) — crate graph and workspace layout
- [RFC 0002](docs/rfcs/0002-storage-trait.md) — Storage trait and Oxigraph backend
- [RFC 0003](docs/rfcs/0003-reasoner-traits.md) — reasoner trait surface
- [RFC 0004](docs/rfcs/0004-context-api.md) — `semanticore-context` API
- [RFC 0005](docs/rfcs/0005-projection-api.md) — `semanticore-projection` API
- RFC 0006 — premium boundary contract (in private companion repo)

For a single-page overview, see [ARCHITECTURE.md](ARCHITECTURE.md).

## What ships in the open tier (Apache 2.0)

- `semanticore-core`; the foundational types, identifiers, and graph primitives every other crate builds on, plus the `Storage` and reasoner traits. Release candidate for 0.1.0.
- `semanticore-store-oxigraph`; the first `Storage` backend (in-memory and file-backed), with a shared contract test suite in the unpublished `semanticore-storage-testkit`.
- `semanticore-context`; the Context OS (formerly Theseus internally); v0.1 skeleton: sessions over enduring, major and minor tiers with promotion and eviction. The OWL 2 EL reasoner, ANN index and SPARQL queries are not implemented yet.
- `semanticore-projection`; bidirectional RDF and labeled-property-graph projection with round-trip metadata and interpretation profiles (formerly Metasemantics internally). Specified, not started.

Bindings: Python via PyO3 (`semanticore-context-py`, abi3 wheel, unpublished) exists; TypeScript via napi-rs is not started. Both ship from the same Rust core so behavior stays identical across runtimes.

Agents and new contributors: see [AGENTS.md](AGENTS.md).

## Premium tier

The premium tier ships distributed and persistent OWL, SHACL, and Datalog reasoners along with EC, MC, and mC engines purpose-built for production-scale Context OS deployments. It targets teams that need horizontal scalability, durable reasoning state, and operational support beyond what a single-process embedded reasoner provides. Distributed via Cloudsmith private registry under a commercial license; contact Enterprise Knowledge for access.

## Status

Pre-1.0. Active development. APIs are unstable.

- Phase 0 — decisions resolved
- Phase 1 — repo + infrastructure scaffolded
- Phase 2 — architecture spec complete (RFCs 0001–0005)
- Phase 3 — open-tier implementation in progress: `semanticore-core` trait surface, the Oxigraph storage backend, the `semanticore-context` skeleton and its Python binding have landed

First release: `semanticore-core` 0.1.0, the storage and reasoner trait surface (SHACL, Datalog, OWL). See [CHANGELOG.md](CHANGELOG.md).

## Install

```sh
cargo add semanticore-core
```

`semanticore-store-oxigraph`, `semanticore-context` and `semanticore-projection` are not yet published (projection is not yet implemented).

```sh
pip install semanticore
```

```sh
npm install @semanticore/core
```

v0.0.1 hello-world available; meaningful 0.1 lands at end of Phase 3.

## License

Licensed under the Apache License, Version 2.0; see [LICENSE](LICENSE).

## Acknowledgments

Architectural inspiration from Equinor's open-source maplib.
