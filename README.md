# semantiCore

A Rust-core, two-tier semantic platform for production knowledge graphs.

semantiCore is a Rust-core project with a two-tier distribution: an open core under Apache 2.0, and a premium tier of distributed reasoners and engines available under a commercial license. Enterprise Knowledge serves as the commercial steward, mirroring the pattern set by Equinor's maplib and Treehouse: open primitives stay open, while production-scale infrastructure ships as a supported product.

## What ships in the open tier (Apache 2.0)

- `semanticore-core`; the foundational types, identifiers, and graph primitives every other crate builds on.
- `semanticore-context`; the Context OS (formerly Theseus internally); context modeling, lifecycle, and resolution.
- `semanticore-projection`; bidirectional RDF and labeled-property-graph projection with round-trip metadata and interpretation profiles (formerly Metasemantics internally).

Bindings: Python via PyO3 and TypeScript via napi-rs are first-class targets; both ship from the same Rust core so behavior stays identical across runtimes.

## Premium tier

The premium tier ships distributed and persistent OWL, SHACL, and Datalog reasoners along with EC, MC, and mC engines purpose-built for production-scale Context OS deployments. It targets teams that need horizontal scalability, durable reasoning state, and operational support beyond what a single-process embedded reasoner provides. Distributed via Cloudsmith private registry under a commercial license; contact Enterprise Knowledge for access.

## Status

Pre-1.0. Active development. APIs are unstable.

## Install

```sh
cargo add semanticore-core
cargo add semanticore-context
cargo add semanticore-projection
```

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
