# semantiCore architecture

> The complete picture lives across a series of RFCs and a public-private repo split.
> This document is a navigation hub. Start here, then follow links.

## Status

Pre-1.0. Active development. APIs unstable. Current phase: Phase 2 — Architecture spec, just closed.

## Project shape

semantiCore is a Rust-core, two-tier reasoning and Context library for agentic applications. The open tier (Apache 2.0) ships `semanticore-core`, `semanticore-context`, `semanticore-projection`, and Python (PyO3) plus TypeScript (napi-rs) bindings; the commercial tier ships premium reasoners (OWL EL/QL/RL/DL, SHACL, Datalog) and distributed Context engines (enduring, major, minor scopes) that drop into the same trait surfaces. The two tiers live in two repositories with a one-way OSS-to-premium dependency rule. Enterprise Knowledge is the commercial steward through 1.0; the architecture is shaped to survive a corporate-structure pivot (spinout, joint venture, foundation contribution) without renaming a crate.

A user with only the OSS tier installed gets a working semantiCore: single-process correct default reasoners, embedded Oxigraph storage, the full Context and Projection APIs. Adding the premium tier swaps in distributed engines and the OWL/SHACL/Datalog reasoners without changing any user-visible API — that is the load-bearing contract this architecture protects.

## Repos

| Repo | Visibility | Purpose | URL |
|---|---|---|---|
| `semanticore` | Public | Apache 2.0 OSS — `semanticore-core`, `semanticore-context`, `semanticore-projection` + bindings | https://github.com/TTTSNB/semanticore |
| `semanticore-premium` | Private | Commercial — premium reasoners, EC/MC/mC engines, license verification | https://github.com/TTTSNB/semanticore-premium |

## Crate map

### Open tier (Apache 2.0)

- `semanticore-core` — shared types (IRIs, terms, triples, quads) and trait definitions (Storage, reasoner traits)
- `semanticore-context` — Context OS (Session, ContextScope, default OSS reasoners)
- `semanticore-projection` — bidirectional RDF↔LPG with round-trip metadata and profile registry
- `semanticore-context-py` / `semanticore-context-ts` — Python (PyO3, sync) and TypeScript (napi-rs, async) bindings
- `semanticore-projection-py` / `semanticore-projection-ts` — projection bindings

### Premium tier (commercial license)

- `semanticore-licensing` — runtime license-key verification; the `Feature` enum and `require_feature` entry point
- `semanticore-reasoner-owl-{el,ql,rl,dl}` — all four OWL profile reasoners (greenfield Rust, ELK/Whelk/tableaux-class)
- `semanticore-reasoner-shacl` — SHACL Core + SHACL-SPARQL
- `semanticore-reasoner-datalog` — stratified Datalog with semi-naive evaluation
- `semanticore-context-engine-{enduring,major,minor}` — distributed and persistent Context engines per scope

Seventeen crates total at 1.0: seven OSS, ten premium. No additional crates are anticipated for the 1.0 release. Maplib feature parity (OTTR, RML, Polars/Arrow, embedded SPARQL), retrieval (`semanticore-retrieval`, formerly KI1-Prometheus), JVM and WASM bindings are explicitly post-1.0.

Full crate inventory, codename mapping, dependency rules, and naming conventions: see [RFC 0001](docs/rfcs/0001-crate-graph.md).

## Reasoning architecture

semantiCore separates trait definitions (open) from production-grade implementations (premium) for every reasoner family. The base traits — `Reasoner`, `Materialization`, `IncrementalReasoner`, `Entailment` — factor out the common inference patterns; specialized traits — `OwlReasoner`, `ShaclValidator`, `DatalogEngine`, `ContextReasoner` — capture the family-specific surfaces. All trait definitions live in `semanticore-core`. Open-source `semanticore-context` ships single-process correctness-focused defaults; premium crates drop in distributed and persistent implementations against the same trait surface without altering a line of application code.

`ReasonerCapabilities` advertises what each implementation can do (persistent, distributed, incremental, etc.). The `Session` selects among linked reasoners using these capabilities, so an application that needs a particular guarantee compiles only when an implementation satisfying it is linked.

Trait surface: see [RFC 0003](docs/rfcs/0003-reasoner-traits.md).

OSS-shipped at v0.1: OWL EL by default only. SHACL, Datalog, and the other OWL profiles (QL/RL/DL) are premium-only. Sessions surface clear `CapabilityShortfall` errors when an unavailable capability is requested, rather than silently degrading.

The reasoner approach is greenfield Rust per the Phase 0 decision: ELK / Whelk-class for OWL EL, tableaux-class for OWL DL, trav-SHACL-class for SHACL, semi-naive evaluation with stratified negation for Datalog, and a fresh design for the three Context-engine scopes. The benchmark target is the union of Treehouse, RDFox, Stardog, GraphDB, and AllegroGraph.

## Context OS overview

`semanticore-context` (internally was Theseus) provides a `Session` over a three-tier memory hierarchy:

- **Enduring Context (EC)** — durable, slowly-changing; the canonical home for ontology axioms, accepted facts, and accumulated user preferences. Survives across runs and Sessions.
- **Major Context (MC)** — working set; warm in memory, session-scoped or scoped to a user/agent/project/task. Holds recent conversation turns, active hypotheses, mid-session decisions.
- **minor Context (mC)** — request-scoped; ephemeral, created and torn down per query or per agent turn. Holds transient inferences and intermediate results.

Each scope has its own `ContextReasoner` impl. OSS defaults are single-process and in-memory; premium engines (`semanticore-context-engine-{enduring,major,minor}`) provide distributed and persistent variants drop-in compatible.

`Session` exposes `add` / `add_batch` / `remove` / `query` / `promote` / `evict` / `close`. `ContextItem` is the unit of content (typed payload + provenance + scope hint). Promotion and demotion move items between scopes as relevance shifts; eviction is bounded and observable.

Public API: see [RFC 0004](docs/rfcs/0004-context-api.md).

## Storage architecture

Pluggable `Storage` trait with a per-transaction `StorageTransaction` GAT and a `StorageCapabilities` advertisement. Default backend at v0.1: Oxigraph (Rust-native, embedded; both in-memory and on-disk variants). GraphDB, Neptune, and Blazegraph adapters are sketched and deferred to Phase 5; reasoners read storage through the trait so swapping the backend is a runtime decision.

The trait pins sync-default with an `AsyncStorage` parallel for HTTP-backed remote stores. The error model uses an associated `Error` type with a default `StorageError` enum carrying `TransactionConflict` (retryable) and `UnsupportedCapability` (refuse-to-run) variants.

Spec: see [RFC 0002](docs/rfcs/0002-storage-trait.md).

## Projection (RDF ↔ LPG)

`semanticore-projection` (internally was Metasemantics) provides bidirectional projection between RDF graphs and labeled property graphs. Round-trip metadata is first-class: with metadata captured during `rdf_to_lpg`, the inverse `lpg_to_rdf` reconstructs the original RDF up to isomorphism on the chosen profile, preserving blank-node identity, datatype provenance, named-graph membership, and language tags.

A `Profile` is a named, versioned bundle of every projection choice — blank-node strategy, datatype strategy, named-graph encoding, RDF-collection form, language-tag strategy, property naming, label assignment. The v0.1 release ships a curated registry covering the dominant LPG dialects (`neo4j-faithful/v1`, `tinkerpop-faithful/v1`, etc.). Teams can register their own profiles at runtime.

Public API: see [RFC 0005](docs/rfcs/0005-projection-api.md).

## Premium boundary

The contract that lets premium evolve in lockstep with OSS without surprise breakage lives in the private premium repo: [RFC 0006](https://github.com/TTTSNB/semanticore-premium/blob/main/docs/rfcs/0006-premium-boundary.md) (private — accessible to TTTSNB and licensed contributors).

Key rule, restated from RFC 0001:

> The `Storage` trait, all reasoner traits, and the Context OS surface live in OSS — premium crates implement them; never define them.

If a premium crate needs a new trait, the trait is added to the appropriate OSS crate first.

Three further rules complete the boundary contract: OSS crates never depend on premium crates (the two repos cannot path-depend on each other, and review enforces the same direction within imports); `semanticore-licensing` is the only premium crate every other premium crate depends on directly, exposing `verify_key`, `require_feature`, and the `Feature` enum; and bindings depend only on their corresponding library crate plus PyO3/napi-rs and serde, never on logic that lives elsewhere.

## Bindings

| Binding | Crate | Approach |
|---|---|---|
| Python | `semanticore-context-py`, `semanticore-projection-py` | PyO3, sync API |
| TypeScript | `semanticore-context-ts`, `semanticore-projection-ts` | napi-rs, async API |

Bindings are intentionally thin: they marshal types and re-export the trait surface. All logic lives in the Rust core where it is testable and reusable. JVM and WASM bindings are non-goals for v0.1.

## How to read this codebase

Recommended order for an outside reader:

1. This file — for orientation
2. [RFC 0001](docs/rfcs/0001-crate-graph.md) — to understand the crate graph and tier split
3. [RFC 0002](docs/rfcs/0002-storage-trait.md) — for the storage abstraction the rest builds on
4. [RFC 0003](docs/rfcs/0003-reasoner-traits.md) — for the reasoner trait surface (the open/premium boundary)
5. [RFC 0004](docs/rfcs/0004-context-api.md) — for the Context OS public API (most users start here)
6. [RFC 0005](docs/rfcs/0005-projection-api.md) — for the projection library (subset of users)
7. The premium boundary contract (RFC 0006) — only for premium contributors

## Phases

Plan-of-record sequenced (no calendar; "grind till it's done"):

| Phase | Status | Description |
|---|---|---|
| 0 | done | Decisions resolved |
| 1 | done | Repo + infra scaffolding |
| 2 | done | Architecture spec (this milestone) |
| 3 | next | Open-tier MVP |
| 4 | | Premium-tier MVP |
| 5 | | Benchmark suite |
| 6 | | Business plan + go-to-market |
| 7 | | 1.0 launch |

Phase 6 (business plan + GTM) runs in parallel from Phase 2 onward.

## Reconciliations from Phase 2

The architecture phase surfaced and resolved five points of inconsistency or open scope. These are recorded here so a reader does not have to reconstruct them from RFC diffs.

1. **Capability words `enduring` / `major` / `minor` are canonical.** EC/MC/mC abbreviations are tolerated in narrative prose; the spelled-out forms are canonical in API symbols (`ContextScope::Enduring`, `ContextScope::Major`, `ContextScope::Minor`) and in documentation headings. RFCs 0001, 0003, and 0004 align on this.

2. **OSS-reasoner scope at v0.1 is OWL EL only.** SHACL and Datalog are premium-only families with no OSS default at v0.1; OWL QL, RL, and DL are also premium-only. Sessions return a `CapabilityShortfall` error rather than silently no-op-ing when an unavailable capability is requested. (RFC 0003; RFC 0004 §"OSS at v0.1: what reasoners ship".)

3. **License-gating canonical error path** is `LicenseError::FeatureNotLicensed(Feature::X)` from `semanticore-licensing`, with `#[from]` conversion into each reasoner-family error type. RFC 0006 locks this contract; T1.6 already implements the source enum.

4. **Tier-to-Feature mapping is decoupled at v0.1.** The `License` carries a `features: Vec<Feature>` directly; the `Tier` enum (`Team` / `Business` / `Enterprise`) is informational only. Server-side enforcement only at v0.1; client-side defense-in-depth deferred. (RFC 0006 Open Question.)

5. **Bindings layout `crates/bindings/...` is aspirational.** Workspace `members` will gain the binding entries during Phase 3 when the binding crates are scaffolded; the current workspace `Cargo.toml` does not yet enumerate them. (RFC 0001 Implementation notes.)

## License

Apache 2.0 for the open tier (this repo).
Commercial license for the premium tier ([semanticore-premium](https://github.com/TTTSNB/semanticore-premium)).

## Acknowledgments

Architectural inspiration from Equinor's open-source maplib. The two-tier OSS + commercial pattern parallels how Treehouse stewards maplib in the commercial tier.
