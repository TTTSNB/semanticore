# semanticore-core

[![Crates.io](https://img.shields.io/crates/v/semanticore-core.svg)](https://crates.io/crates/semanticore-core)
[![Docs.rs](https://docs.rs/semanticore-core/badge.svg)](https://docs.rs/semanticore-core)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)

The open-source trait surface for the [semantiCore](https://github.com/TTTSNB/semanticore)
Rust ecosystem.

`semanticore-core` **defines** the cross-cutting types and traits that the rest of
the ecosystem compiles against; it does not ship a reasoning engine itself. It is
the stable, permissively-licensed boundary that engine implementations — open or
closed — implement. Premium engines (the `semanticore-reasoner-*` family) depend
on this crate and implement its traits; they never define them.

## What's in it

- **`model`** — the canonical RDF term model: `NamedNode`, `BlankNode`, `Literal`,
  `Term`, and `Quad` (subject / predicate / object / graph).
- **`storage`** — the `Storage` and `StorageTransaction` traits for backing a graph
  store.
- **`reasoner`** — the `Reasoner` base trait plus the `Materialization`,
  `IncrementalReasoner`, `Entailment`, and `OwlReasoner` capability traits, and the
  capability-advertisement vocabulary.
- **`shacl`** — the `ShaclValidator` trait and validation-report vocabulary.
- **`datalog`** — the `DatalogEngine` trait, program IR, and result types.
- **`owl`** — the OWL reasoner return types and error family.

These are the traits the SHACL, Datalog, and OWL engines implement. Because the
trait surface lives here under Apache-2.0, the engines can stay independent and the
core stays independently publishable.

## Install

```toml
[dependencies]
semanticore-core = "0.1"
```

## Usage

`semanticore-core` is a trait + model crate, so the typical use is constructing RDF
terms and implementing one of the traits for your own engine or store. Building a
quad:

```rust
use semanticore_core::model::{Literal, NamedNode, Quad, Term};

let subject = NamedNode::new("https://example.org/alice");
let predicate = NamedNode::new("http://xmlns.com/foaf/0.1/name");
let object = Term::Literal(Literal::new_simple("Alice"));

let quad = Quad::triple(subject.into(), predicate, object);
assert_eq!(quad.predicate.as_str(), "http://xmlns.com/foaf/0.1/name");
```

Implementing the reasoner surface for your own engine:

```rust,ignore
use semanticore_core::shacl::ShaclValidator;
// impl ShaclValidator for MyEngine { /* ... */ }
```

## Links

- Documentation: <https://docs.rs/semanticore-core/>
- Source: <https://github.com/TTTSNB/semanticore>

## License

Licensed under the [Apache License, Version 2.0](./LICENSE).
