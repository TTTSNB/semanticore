# RFC 0005: semanticore-projection API surface

- **Status:** Accepted
- **Date:** 2026-05-07
- **Author:** Elliott Risch
- **Phase:** 2 — Architecture spec
- **Depends on:** RFC 0001
- **Crate:** `semanticore-projection`

## Summary

This RFC fixes the public API of `semanticore-projection`, the OSS crate that provides bidirectional translation between RDF and labeled property graph (LPG) representations. The API is built around three first-class concepts: a `Projection` value that owns a profile and accumulates `RoundTripMetadata`, a `Profile` that names and versions every projection choice (blank-node strategy, datatype handling, named-graph encoding, language tags, RDF-collection form, property naming, label assignment), and a `ProfileRegistry` that lets teams publish and resolve named profiles by ID. Round-trip survivability is the load-bearing differentiator: with the metadata captured during `rdf_to_lpg`, the inverse `lpg_to_rdf` reconstructs the original RDF graph up to isomorphism on the chosen profile. Bindings ship for Python (PyO3) and TypeScript (napi-rs).

## Motivation

RDF and LPG are not equivalent data models. RDF is a triple/quad store with global IRIs, blank nodes, datatype-tagged literals, language tags, and (in RDF 1.1) named graphs; LPG is a multi-graph of nodes and edges with arbitrary property maps and labels. Translating between them is unavoidable in practice — RDF is the lingua franca of formal vocabularies and reasoning, while LPG is the dominant model for graph databases like Neo4j and TinkerPop and for visualization tools like Gephi or yWorks. Most teams reach for an ad-hoc converter, hardcode their preferences, and discover months later that the mapping is lossy in a way they cannot un-pick.

A separate, focused library is justified because the design space is genuinely large. There are at least seven independent decisions any RDF→LPG mapper must make (blank nodes, datatypes, named graphs, RDF collections, language tags, property naming, label assignment), and each decision has multiple defensible answers depending on the downstream consumer. A faithful Neo4j projection that preserves multi-edges looks nothing like a flat visualization projection that collapses them. Bundling those decisions into a named, versioned `Profile` makes the choices explicit, declarable in configuration, and stable across releases. A team can write `profile = "neo4j-faithful/v1"` in their pipeline config and have a guarantee that the projection behavior is fixed for the lifetime of that profile version.

Round-trip survivability is the differentiator. Most converters in the wild are one-way: RDF in, LPG out, never the reverse. That is fine for a one-shot ETL but unacceptable for any pipeline that wants to materialize a knowledge graph in Neo4j, allow edits there, and round-trip the changes back to RDF for reasoning. The only way to make that round-trip lossless is to capture the projection-time decisions as metadata that the inverse can consult. `RoundTripMetadata` is that capture: it remembers original blank-node identities, original datatypes, named-graph membership, RDF-collection form, and language tags. With the metadata, the inverse is exact; without it, the inverse is best-effort, and which guarantees still hold is a property of the profile.

A first-class `ProfileRegistry` lets teams declare, version, and share their projection choices. Profiles are not opaque; they are inspectable values with a stable schema, a SemVer version, and a registry that supports lookup by ID. Two teams collaborating on a graph pipeline can agree on `acme-internal/v3` and know they are projecting the same way without re-deriving the choices from prose. New profiles register at runtime; the v0.1 release ships a curated set covering the dominant LPG dialects.

## Top-level concepts

- **RDF model:** triples and quads, IRIs and blank nodes as subjects/predicates/objects, datatype-tagged literals, language-tagged literals, named graphs, RDF-star quoted triples (optional, profile-dependent).
- **LPG model:** nodes (with labels and a property map), edges (typed, directed, with a property map; multi-edges between the same pair allowed), graph-level metadata.
- **Projection:** a function pair (`rdf_to_lpg`, `lpg_to_rdf`) producing an isomorphism modulo a chosen profile. The two functions are dual under the profile's choices and the captured metadata.
- **Round-trip metadata:** information attached during projection that the inverse needs to restore the original RDF — blank-node identity, datatype provenance, named-graph membership, language-tag survival, RDF-collection vs. multi-edge encoding choices.
- **Interpretation profile:** a named, versioned bundle of every choice that determines projection behavior. The pair (profile, metadata) fully specifies the projection — given both, the inverse is deterministic.
- **Profile compatibility:** a relation between two profiles describing whether a projection forward through profile X followed by an inverse through profile Y is lossless. The compatibility matrix is part of the v0.1 release.

## Public API surface

### `Projection` (top-level type)

```rust
pub struct Projection {
    profile: Profile,
    metadata: RoundTripMetadata,
}

impl Projection {
    pub fn new(profile: Profile) -> Self;
    pub fn with_metadata(profile: Profile, metadata: RoundTripMetadata) -> Self;

    pub fn rdf_to_lpg<S, L>(&mut self, source: &S, sink: &mut L) -> Result<ProjectionStats, ProjectionError>
    where S: RdfSource, L: LpgSink;

    pub fn lpg_to_rdf<L, R>(&self, source: &L, sink: &mut R) -> Result<ProjectionStats, ProjectionError>
    where L: LpgSource, R: RdfSink;

    pub fn round_trip_metadata(&self) -> &RoundTripMetadata;
}
```

`Projection::rdf_to_lpg` takes `&mut self` because it accumulates into `self.metadata` as it walks the input. `lpg_to_rdf` takes `&self` because it only reads. `ProjectionStats` reports counts (triples consumed, nodes/edges produced, properties emitted, blank nodes encountered, etc.) so callers can sanity-check large pipelines without instrumenting every call site themselves.

The generic `RdfSource` / `RdfSink` / `LpgSource` / `LpgSink` traits decouple `Projection` from any specific store. `RdfSource` is implementable by `oxigraph::Store`, by an in-memory `Vec<Quad>`, by a streaming reader; `LpgSink` is implementable by a Neo4j driver wrapper, by a TinkerPop bulk loader, by an in-memory graph. The traits live in `semanticore-projection::io` and are stable from v0.1.

### `Profile` and `ProfileRegistry`

```rust
pub struct Profile {
    pub id: ProfileId,           // e.g. "neo4j-faithful/v1"
    pub version: semver::Version,
    pub blank_node_strategy: BlankNodeStrategy,
    pub datatype_strategy: DatatypeStrategy,
    pub named_graph_strategy: NamedGraphStrategy,
    pub collection_strategy: CollectionStrategy,
    pub language_tag_strategy: LanguageTagStrategy,
    pub property_naming: PropertyNamingScheme,
    pub label_strategy: LabelStrategy,
}

pub struct ProfileRegistry { /* opaque */ }

impl ProfileRegistry {
    pub fn global() -> &'static ProfileRegistry;
    pub fn register(&self, profile: Profile) -> Result<(), RegistryError>;
    pub fn resolve(&self, id: &ProfileId) -> Option<Profile>;
    pub fn list(&self) -> Vec<ProfileId>;
}
```

Each strategy enum has a small, finite set of variants. For example, `BlankNodeStrategy` is one of `{Skolemize, PreserveAsProperty, GenerateUuid}`; `DatatypeStrategy` is one of `{TypedProperty, StringWithProvenance, NarrowToNative}`; `NamedGraphStrategy` is one of `{EdgeProperty, NodePartition, Ignore}`; `CollectionStrategy` is one of `{MultiEdge, ChainedNodes, JsonArrayProperty}`; `LanguageTagStrategy` is one of `{SuffixedKey, NestedProperty, DropTag}`. Adding new variants to a strategy enum is a SemVer breaking change for the profile schema and triggers a major-version bump on every profile that consumes it.

`ProfileId` is a parsed string `"<name>/v<major>"`. The version field on `Profile` carries the full SemVer, but the `ProfileId` only carries the major because two minor versions of the same profile must produce identical projections by definition (a minor version is reserved for backwards-compatible additions like new strategy variants the profile does not exercise).

`ProfileRegistry::global()` returns a process-wide registry pre-populated with the four default profiles enumerated below. Custom profiles register at startup; concurrent registration is safe (the registry uses interior mutability with a fast-read lock).

### `RoundTripMetadata`

```rust
pub struct RoundTripMetadata {
    pub original_blank_node_ids: BTreeMap<NodeId, BlankNodeId>,
    pub datatype_provenance: BTreeMap<EdgePropertyKey, NamedNode>,
    pub graph_membership: BTreeMap<TermId, Vec<NamedGraphId>>,
    pub original_collection_form: BTreeMap<NodeId, CollectionForm>,
    pub language_tags: BTreeMap<EdgePropertyKey, String>,
}

impl RoundTripMetadata {
    pub fn empty() -> Self;
    pub fn merge(&mut self, other: Self);
    pub fn is_lossless(&self) -> bool;
    pub fn to_json(&self) -> String;
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error>;
}
```

`BTreeMap` is used (not `HashMap`) so the JSON serialization is deterministic key-ordered, which matters when metadata is checked into a pipeline as an artifact and diffed across runs. `is_lossless` returns true iff every projection-time decision was captured in metadata; for a lossy profile (see `flat-graph/v1` below), it always returns false. `merge` combines metadata from two projections of disjoint inputs — useful when a pipeline shards its RDF input and projects each shard in parallel.

JSON is the only canonical serialization. The schema is documented and stable from v0.1 forward; consumers can write their own readers in any language without depending on the Rust crate.

## Round-trip guarantees

Three guarantees are stated explicitly so consumers can reason about correctness without reading the source:

1. **Lossless round-trip when metadata is preserved.** For any RDF graph A, profile P, and the metadata M captured during `Projection::new(P).rdf_to_lpg(A, &mut B)`, the subsequent call `Projection::with_metadata(P, M).lpg_to_rdf(B, &mut A')` produces an A' that is isomorphic to A under the equivalence relation defined by P. Isomorphism here means: identical IRI subjects/predicates/objects, identical literal values up to datatype-equivalent canonicalization, identical blank-node graph structure (modulo blank-node renaming), identical named-graph membership, identical RDF-collection structure where preserved.
2. **Metadata-free round-trip is profile-dependent.** Without preserved metadata, lossless reconstruction is only guaranteed for profiles that explicitly declare it (none of the v0.1 profiles do, because every realistic profile loses at least blank-node identity without metadata). For all v0.1 profiles, a metadata-free inverse is best-effort; the resulting RDF is a valid projection-inverse but not isomorphic to the original.
3. **Profile compatibility.** Two profiles P and Q are compatible iff `rdf_to_lpg` under P followed by `lpg_to_rdf` under Q is lossless given P's metadata. The v0.1 compatibility matrix is small: every profile is compatible with itself; `neo4j-faithful/v1` and `tinkerpop-faithful/v1` are pairwise compatible (they differ only in property-naming convention, which the metadata records); `flat-graph/v1` is incompatible with everything except itself; `rdf-star/v1` is compatible only with itself.

## Default profiles shipped with v0.1

| Profile ID | Description | Round-trip lossless? |
|---|---|---|
| `neo4j-faithful/v1` | Optimized for Neo4j Cypher; preserves multi-edges, named graphs as edge property | Yes (with metadata) |
| `tinkerpop-faithful/v1` | Apache TinkerPop / Gremlin compatible; supports multi-properties | Yes (with metadata) |
| `flat-graph/v1` | Lossy; one property per edge; for visual graph tools | No |
| `rdf-star/v1` | Preserves RDF-star quoted triples as nested LPG nodes | Yes (with metadata) |

`neo4j-faithful/v1` uses `BlankNodeStrategy::Skolemize` (blank nodes become Skolem-IRI nodes), `DatatypeStrategy::TypedProperty` (datatypes encode in property type or in metadata), `NamedGraphStrategy::EdgeProperty` (named graph IRI attached as an edge property `_g`), `CollectionStrategy::MultiEdge` (RDF lists become multi-edges with index property), `LanguageTagStrategy::SuffixedKey` (`label@en` becomes property `label_en` with metadata recording the original tag), `PropertyNamingScheme::LastSegment`, `LabelStrategy::FromRdfType`.

`tinkerpop-faithful/v1` differs from `neo4j-faithful/v1` only in `PropertyNamingScheme::FullIri` (TinkerPop tolerates colons in keys) and `LanguageTagStrategy::NestedProperty` (TinkerPop multi-properties make nested values cleaner). Compatibility with `neo4j-faithful/v1` holds because both differences are captured in metadata.

`flat-graph/v1` is the lossy fallback. `BlankNodeStrategy::GenerateUuid` (no Skolemization, no metadata), `DatatypeStrategy::NarrowToNative` (everything becomes string/int/float), `NamedGraphStrategy::Ignore`, `CollectionStrategy::JsonArrayProperty`, `LanguageTagStrategy::DropTag`. This profile is for piping RDF into Gephi or yWorks for one-shot visualization; it is documented as not round-trippable.

`rdf-star/v1` is the only profile that handles RDF-star quoted triples (RDF 1.2 / SPARQL-star). Quoted triples become nested LPG nodes with edges back to their constituent terms. Datatype, blank-node, named-graph, and collection strategies are inherited from `neo4j-faithful/v1`; the addition is the quoted-triple handling.

## Bindings story

**Python (PyO3).** The Python binding lives in `semanticore-projection-py` and ships on PyPI as `semanticore_projection_py`. The surface mirrors the Rust API: `Projection(profile_id="neo4j-faithful/v1")` constructs a projection by resolving the ID against the global registry; `projection.rdf_to_lpg(source, sink)` and `projection.lpg_to_rdf(source, sink)` take any object exposing the documented iterator protocol; `RoundTripMetadata` exposes a `to_dict` / `from_dict` round-trip so Python users can pickle metadata or pass it through Pandas. `Profile` and `ProfileRegistry` are exposed as opaque handles; custom profiles are constructed via `Profile.builder()` to keep the kw-arg surface manageable. Errors map to a hierarchy rooted at `ProjectionError(Exception)`.

**TypeScript (napi-rs).** The TS binding lives in `semanticore-projection-ts` and ships on npm as `@semanticore/projection`. The surface is async-friendly: every method that walks a large input returns a Promise (the Rust core runs on a Tokio worker pool; napi-rs handles the marshaling). `import { Projection, ProfileRegistry, RoundTripMetadata } from "@semanticore/projection";`. `Projection.rdfToLpg(source, sink)` resolves with `ProjectionStats`. `RoundTripMetadata` serializes to/from plain objects via `toJSON` / `fromJSON`. Type definitions ship in the package; consumers get full IntelliSense without a separate `@types` install.

Both bindings depend only on `semanticore-projection`, PyO3 / napi-rs, and serde. They contain no logic — every projection decision lives in the Rust core where it is testable once and reused everywhere. This is the same binding-as-marshaling rule established in RFC 0001.

## Migration notes

`semanticore-projection` is a greenfield rewrite. An early internal prototype (referenced in chat history under a different name; the codename mapping is recorded in RFC 0001) was never published to crates.io, never depended on by external consumers, and shares no code with this crate. There is no compatibility shim and no deprecation period. Any internal consumers of the prior prototype rewrite against this API. The migration is not painful in practice because the prior prototype's surface area was small and the round-trip metadata concept is new in this rewrite.

## Open questions

- **Schema export from interpretation profiles.** It would be useful to derive a Cypher `CREATE CONSTRAINT` script or a Neo4j schema file from a profile, so that LPG sinks materialize with the right indexes and uniqueness constraints. The signature is straightforward (`profile.export_schema(target: SchemaTarget) -> String`) but the per-target output formats are large enough to defer to a follow-up RFC.
- **Stream-based projection for graphs that don't fit in memory.** The current `RdfSource` / `LpgSink` traits are pull-based but materialize one quad / edge at a time. True streaming with bounded memory for very large graphs (10^9+ triples) requires a different shape — likely a windowed-batch protocol with explicit blank-node-scope flushing. Deferred to v0.2.
- **Cypher-subset query layer integrated with `semanticore-context` query surface.** Once an LPG view exists, exposing a small Cypher subset as an alternative to SPARQL on the same data would let consumers query in either model. This is more naturally a `semanticore-context` feature (the query surface is already there) and is deferred to that crate's roadmap.
- **Property-graph constraint-checking analogous to SHACL on RDF.** SHACL gives RDF a constraint language; LPG does not have a standardized analogue (Neo4j has `CREATE CONSTRAINT`, TinkerPop has nothing comparable). Whether constraint-checking belongs in `semanticore-projection`, in a new sibling crate, or merged into the reasoner story (RFC 0003) is undecided. Deferred until the reasoner work is further along.
