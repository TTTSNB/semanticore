# RFC 0004: semanticore-context API surface

- **Status:** Accepted
- **Date:** 2026-05-07
- **Author:** Elliott Risch
- **Phase:** 2 — Architecture spec
- **Depends on:** RFC 0001, RFC 0002, RFC 0003
- **Crate:** `semanticore-context`

## Summary

This RFC fixes the public API of `semanticore-context`, the OSS Context OS library at the spine of every agentic application built on semantiCore. The crate exposes a `Session` that opens a graph-backed context store, configures a `ContextReasoner` per scope, and serves queries, updates, and lifecycle operations against a three-tier memory hierarchy: enduring context (persistent, slowly-changing), major context (working-set, session-lived), and minor context (request-scoped, ephemeral). The `ContextScope` discriminator from RFC 0003 is the load-bearing primitive that lets the same Session route work to different reasoners with different durability and scale characteristics. v0.1 ships default OSS reasoners only — single-process, in-memory, correctness-focused — covering the OWL 2 EL profile; SHACL and Datalog operations are surfaced through the API but refused at runtime unless a premium reasoner is linked. Premium drop-ins (`semanticore-context-engine-{enduring,major,minor}`) replace any of the three scopes with distributed, persistent, multi-tenant implementations without changing a line of application code. Bindings for Python (PyO3, sync) and TypeScript (napi-rs, async) re-export the full surface.

## Motivation

A Context library separate from raw graph storage is justified by the gap between what an agent needs and what a triple store provides. A triple store gives you quads, transactions, and SPARQL; an agent needs ranked recall, scope-aware retention, promotion of useful inferences, eviction of stale ones, and a single object that can be opened, queried, and closed across a turn. Pushing that intelligence into application code — the path the legacy SDK took — left every agent author re-implementing the same retention and scoring loop, getting it subtly wrong, and bleeding correctness through the cracks. A first-class Context surface compresses the agent loop into one type with one lifecycle.

The agent ergonomics story is the second motivation. Every realistic agentic application needs three memory tiers: a long-term store of stable knowledge (an ontology, a corpus of accepted facts, accumulated user preferences), a working-set store that holds the current conversation or task (recent turns, active hypotheses, mid-session decisions), and a per-turn scratch space for transient inferences that should not survive the next turn. Naming those tiers as `Enduring`, `Major`, and `minor` Context, and making them peers of one another in the API, gives application code a stable vocabulary that matches how agent developers already reason about state.

This is not RAG. RAG is one query pattern (embed, retrieve top-k, stuff into prompt) over an opaque vector index. The Context OS does that pattern when called for, but it also handles structured RDF facts, typed events with provenance, deductive entailments produced by linked reasoners, and explicit promotion / demotion / eviction lifecycle operations that no RAG store exposes. Treating the Context surface as a graph-first, agent-aware API rather than a retrieval primitive is the architectural commitment that makes everything downstream — the reasoner trait integration, the round-trippable projection API, the premium-tier drop-ins — fit together.

The trait-based reasoner boundary matters because the durability and scale characteristics that distinguish OSS from premium are not implementation details a user can paper over. A single-process EL reasoner saturating an in-memory enduring store is correct and useful at one scale; a sharded, persistent, multi-tenant enduring engine is correct and useful at a different scale. The trait surface from RFC 0003 lets the Session hold either one through the same reference, lets `ReasonerSelection::Auto` pick the right impl at runtime, and lets `ReasonerSelection::Explicit` pin a specific instance when the application has a stronger opinion. Above the trait surface, no API ever changes when a reasoner is swapped — that is the contract this RFC formalizes.

## Top-level concepts

- **Session** — the unit of access. A `Session` opens a context store backed by one or more `Storage` instances (RFC 0002), configures a `ContextReasoner` per scope (RFC 0003), and serves queries and updates against the configured tiers. A Session has a lifecycle: `open` → use → `close`. Every public Context operation goes through a Session.

- **Enduring Context (EC)** — the persistent, slowly-changing tier. Durable across runs and across Sessions; the canonical home for ontology axioms, accepted facts, accumulated user preferences, and any knowledge intended to survive the agent process. v0.1 OSS backs this with a single-process Oxigraph store; premium ships `semanticore-context-engine-enduring` for distributed and persistent backing.

- **Major Context (MC)** — the working-set tier. Warm in memory; survives a Session but is typically scoped to a user, agent, project, or active task. Holds recent conversation turns, active hypotheses, mid-session decisions, and any state that is too volatile to enter the enduring tier but too valuable to discard between turns. v0.1 OSS backs this with an in-memory Oxigraph store; premium ships `semanticore-context-engine-major` for cross-process and federated working sets.

- **minor Context (mC)** — the request-scoped tier. Ephemeral; created and torn down per query or per agent turn. Holds transient inferences, query-time entailments, and intermediate results that should not survive the next turn. v0.1 OSS backs this with a transient working buffer; premium ships `semanticore-context-engine-minor` for high-throughput per-request reasoning.

- **ContextItem** — the unit of content. A typed payload (RDF triples, a structured event, an embedded chunk) plus provenance plus an explicit scope hint. ContextItem is what the application adds to and removes from the Session; the Session routes each item to the storage tier matching its scope.

- **Promotion / demotion** — how items move between scopes as relevance shifts. An mC inference that proves repeatedly useful within a window auto-promotes to MC; an MC fact referenced across Sessions auto-promotes to EC; explicit promotion and demotion are also exposed via `Session::promote`. Demotion is triggered by eviction policy when a tier exceeds its bound, and items selected for demotion either move to a colder tier or are evicted entirely depending on configuration.

- **Eviction** — bounded resource use within each scope. Each tier has a configurable size bound (`ScopeBounds`); when the bound is exceeded, the configured `EvictionPolicy` (LRU, LFU, recency-weighted) selects items to evict. Eviction is observable: `Session::evict` returns the count of items removed, and the eviction events flow through the Session's observability hooks.

The terms `enduring`, `major`, and `minor` are the public capability names per the RFC 0001 Codename Stripping Map. The abbreviations EC, MC, and mC are acceptable in compact prose and in this RFC's narrative sections, but the spelled-out forms are canonical in API names — `ContextScope::Enduring`, `ContextScope::Major`, `ContextScope::Minor` — and in every public symbol that appears in source.

## Public API surface

### `Session`

```rust
pub struct Session<S: Storage, R: ContextReasoner> {
    storage: S,
    reasoners: ContextReasonerSet<R>,
    config: SessionConfig,
}

impl<S: Storage, R: ContextReasoner> Session<S, R> {
    /// Open a Session. Configures storage, registers reasoners, sets bounds
    /// and eviction policy. Returns an error if storage cannot be opened
    /// or if the configured reasoner lineup cannot satisfy the bounds.
    pub fn open(config: SessionConfig) -> Result<Self, SessionError>;

    /// Close the Session. Flushes any pending writes to the enduring
    /// storage, drops the major and minor tiers, and returns final stats.
    pub fn close(self) -> Result<SessionStats, SessionError>;

    /// Add a single item to the named scope. Returns the assigned ItemId.
    pub fn add(&mut self, scope: ContextScope, item: ContextItem)
        -> Result<ItemId, SessionError>;

    /// Add a batch of items to the named scope. More efficient than
    /// calling `add` in a loop for large batches.
    pub fn add_batch(&mut self, scope: ContextScope, items: &[ContextItem])
        -> Result<Vec<ItemId>, SessionError>;

    /// Remove an item by id. Returns true if the item was present and
    /// removed; false if it was not in the named scope.
    pub fn remove(&mut self, scope: ContextScope, id: &ItemId)
        -> Result<bool, SessionError>;

    /// Run a query against the configured tiers. The query DSL determines
    /// which scopes are consulted; see `ContextQuery` below.
    pub fn query(&self, query: ContextQuery)
        -> Result<ContextQueryResult, SessionError>;

    /// Run reasoning over a single scope. Materializes entailments under
    /// the registered reasoner for that scope and writes them back into
    /// the scope's storage.
    pub fn reason(&mut self, scope: ContextScope)
        -> Result<ReasoningStats, SessionError>;

    /// Promote a set of items from one scope to another. The receiving
    /// scope must be of equal or greater durability; demotion is via
    /// `evict` with `EvictionCriteria::Demote`.
    pub fn promote(
        &mut self,
        items: &[ItemId],
        from: ContextScope,
        to: ContextScope,
    ) -> Result<PromotionStats, SessionError>;

    /// Evict items from a scope according to criteria. Returns the count
    /// removed. Eviction may demote (move to a colder scope) rather than
    /// drop entirely, depending on the SessionConfig.
    pub fn evict(&mut self, scope: ContextScope, criteria: EvictionCriteria)
        -> Result<u64, SessionError>;

    /// Capture a snapshot of the named scope. Snapshots are used for
    /// observability, debugging, and (in premium engines) cross-process
    /// state transfer.
    pub fn snapshot(&self, scope: ContextScope)
        -> Result<ContextSnapshot, SessionError>;
}
```

`Session` is generic over both `Storage` (RFC 0002) and `ContextReasoner` (RFC 0003) so the compiler enforces that the registered reasoners are compatible with the registered storage backends. In practice the bindings (and most application code) work through a `BoxedSession` type alias that erases both generics behind trait objects; the generic form is exposed for users who want zero-cost dispatch.

`Session::open` and `Session::close` form an explicit lifecycle. `open` is fallible because storage may not exist on disk yet, the configured reasoner may not be linked, or the configured bounds may exceed available memory. `close` is fallible because flushing pending writes to enduring storage may fail; applications that cannot afford to lose unflushed state must check the result. The returned `SessionStats` reports total items added, queries served, reasoning passes performed, and bytes flushed — the canonical observability summary for a Session's run.

`Session::add` and `Session::add_batch` are scope-explicit: the application names where each item goes. `Session::add` is the convenient single-item entry; `add_batch` is mandatory when ingesting more than a handful of items, because it avoids one transaction per item against the underlying storage. Both return `ItemId` values, opaque per-Session handles that subsequent calls (`remove`, `promote`) reference. ItemIds are stable for the lifetime of a Session; they are not stable across Session restarts (use the item's IRI or a content hash for cross-Session identity).

`Session::query` accepts a `ContextQuery` (defined below) and returns a `ContextQueryResult`. The query DSL determines which scopes are consulted, in what order, and how their results are merged. This is the dominant entry point for retrieval-driven workloads.

`Session::reason` triggers materialization for a single scope. The Session looks up the registered `ContextReasoner` for that scope (RFC 0003) and invokes its materialization or entailment surface against the scope's storage. Reasoning is on-demand, not automatic — applications that want continuous materialization configure a background tick (deferred to v0.2; see Open questions).

### `SessionConfig`

```rust
pub struct SessionConfig {
    /// Storage backend(s) and paths. v0.1 ships a single-storage shorthand
    /// (one Storage instance shared across all three scopes); v0.2 adds a
    /// multi-storage shape (one Storage per scope).
    pub storage: StorageConfig,

    /// How reasoners are selected for each scope.
    pub reasoners: ReasonerSelection,

    /// Per-scope size bounds. Exceeding a bound triggers eviction.
    pub bounds: ScopeBounds,

    /// Per-scope eviction policy.
    pub eviction_policy: EvictionPolicy,

    /// Persistence target for the enduring tier.
    pub persistence: PersistenceConfig,
}

pub enum ReasonerSelection {
    /// Pick the best available reasoner for each scope, OSS or premium,
    /// using the algorithm in "Reasoner integration" below.
    Auto,
    /// Pin a specific reasoner for each scope. Used when an application
    /// has stronger opinions than the auto-selector.
    Explicit(ReasonerLineup),
}

pub struct ReasonerLineup {
    pub enduring: Box<dyn ContextReasoner>,
    pub major: Box<dyn ContextReasoner>,
    pub minor: Box<dyn ContextReasoner>,
}

pub struct ScopeBounds {
    pub enduring_max_items: Option<u64>,
    pub major_max_items: Option<u64>,
    pub minor_max_items: Option<u64>,
}

pub enum EvictionPolicy {
    Lru,
    Lfu,
    RecencyWeighted { half_life: std::time::Duration },
}

pub struct PersistenceConfig {
    /// Where to back the enduring tier.
    pub target: PersistenceTarget,
    /// How aggressively to flush.
    pub flush_strategy: FlushStrategy,
}
```

`ReasonerSelection::Auto` is the default and the right answer for almost every application. `Explicit` exists because there are realistic cases where an application wants to pin a particular reasoner — e.g., always use the premium DL reasoner even on EL workloads to avoid the routing overhead, or run two Sessions in the same process with different reasoner choices for A/B comparison.

`ScopeBounds` uses `Option<u64>` so an unbounded tier is expressible (`None`); applications with very small contexts may run unbounded for all three tiers. Bounds are advisory in the sense that exceeding them is not a hard error — they trigger eviction. Hard errors come from `ReasonerCapabilities::max_axioms` (RFC 0003), which is per-reasoner and inviolable.

### `ContextItem`, `ContextQuery`, `ContextQueryResult`

```rust
pub struct ContextItem {
    /// The payload. One of: a set of RDF triples, a structured event,
    /// or an embedded chunk with vector data.
    pub payload: ContextPayload,
    /// Where this item came from. Required; the Session refuses to add
    /// items without provenance.
    pub provenance: Provenance,
    /// Suggested scope. May be overridden at `add` time; the Session
    /// records the effective scope on the resulting ItemId.
    pub scope_hint: Option<ContextScope>,
    /// Optional embedding for similarity search.
    pub embedding: Option<Vec<f32>>,
}

pub enum ContextPayload {
    /// One or more RDF triples. The triples are inserted into the
    /// scope's storage with the item's provenance attached as graph
    /// metadata.
    Triples(Vec<Quad>),
    /// A typed structured event with a JSON-encoded body. Events are
    /// canonical for agent turns, tool invocations, and system signals
    /// that do not naturally map to triples.
    Event { kind: String, body: serde_json::Value },
    /// A chunk of text or other content, optionally with a precomputed
    /// embedding. Chunks support similarity search.
    Chunk { content: String, content_type: String },
}

pub struct ContextQuery {
    /// Which scopes to consult and how to merge their results.
    pub scopes: ScopeSelection,
    /// The actual query.
    pub kind: ContextQueryKind,
    /// Maximum results to return.
    pub limit: Option<usize>,
    /// Optional reasoner to apply at query time.
    pub reasoner_hint: Option<ContextScope>,
}

pub enum ScopeSelection {
    /// Consult one scope only.
    Single(ContextScope),
    /// Consult all three scopes; merge by score.
    All,
    /// Consult an explicit set of scopes.
    Set(Vec<ContextScope>),
}

pub enum ContextQueryKind {
    /// Pattern matching: an RDF quad pattern or a SPARQL query string.
    Pattern(QuadPattern),
    Sparql(String),
    /// Similarity search over embedded chunks. Returns ranked items.
    Similarity { query_embedding: Vec<f32>, top_k: usize },
    /// Reasoner-augmented inference: a query that asks the registered
    /// reasoner to entail the answer.
    Inference(InferenceQuery),
}

pub struct ContextQueryResult {
    pub items: Vec<RankedItem>,
    pub query_stats: QueryStats,
}

pub struct RankedItem {
    pub id: ItemId,
    pub item: ContextItem,
    pub score: f64,
    pub source_scope: ContextScope,
}
```

`ContextItem` is the unit of content. Provenance is required because Context items participate in reasoning, and reasoning over data without provenance produces conclusions the application cannot defend. The `scope_hint` field is a hint, not a directive — the Session may route the item to a different scope based on the item's payload (a large embedded chunk that overflows the requested minor tier might land in major instead). The effective scope is recorded in the ItemId returned from `add`.

`ContextQuery` is the query DSL. v0.1 ships four query kinds: pattern matching against RDF quads, SPARQL string execution (delegated to the underlying `Storage` if it advertises `sparql_endpoint=true` per RFC 0002, otherwise evaluated on top of `quads_for_pattern`), similarity search over embedded chunks (delegated to a default ANN backend at v0.1; see Open questions), and reasoner-augmented inference (the registered `ContextReasoner` for the named scope is invoked via `Entailment::entails` or `entailed_quads` from RFC 0003). Future query kinds are additive; the enum is `#[non_exhaustive]`.

`ContextQueryResult` returns ranked items. Scoring is a function of the query kind: pattern matches return all hits with score 1.0, similarity searches return cosine similarity scores, inference queries return the registered reasoner's confidence (1.0 when the reasoner is monotone, between 0 and 1 when the reasoner provides explanation depth as a confidence proxy). The `source_scope` field tells the application which tier produced the item, useful for debugging and for displaying provenance to end users.

### Promotion / demotion semantics

Promotion is how items move between tiers as the agent's understanding of their value evolves. The Session tracks two signals per item: access frequency within a window, and reference count from other items (an item that other items cite is more central than one cited by nothing).

Auto-promotion runs on a configurable cadence (default: at the end of each `Session::query` call):

- **mC → MC** when an item is accessed more than `promotion_threshold_minor_to_major` times within `promotion_window_minor`. Default: 3 accesses within 60 seconds.
- **MC → EC** when an item is referenced from more than `promotion_threshold_major_to_enduring` other items, or when it has been accessed across more than `promotion_session_threshold` distinct Sessions. Default: 5 references, or 3 distinct Sessions.

Auto-demotion runs when a tier exceeds its `ScopeBounds`. The configured `EvictionPolicy` selects items; depending on `SessionConfig::eviction_policy.demotion_target`, evicted items either drop entirely or move to the next-colder tier. Demotion to a colder tier is the default for items recently promoted (a one-step policy that prevents thrashing); drop-on-evict is the default for items that have lived in a tier through multiple promotion cycles.

Explicit promotion via `Session::promote` is always allowed, subject to the durability constraint: the receiving scope must be of equal or greater durability than the source scope (Minor → Major → Enduring is allowed; Enduring → Minor is not — demotion in that direction is via `evict` with `EvictionCriteria::Demote`). Explicit promotion bypasses the auto-promotion thresholds and is the canonical way for an application that has external knowledge ("the user just confirmed this fact, promote it to enduring") to drive lifecycle.

## Reasoner integration

The `Session` holds three `ContextReasoner` impls — one per scope — and routes every reasoning operation to the impl that owns the operation's scope. This is the load-bearing integration point with RFC 0003.

- **Default OSS impls** ship inside `semanticore-context` itself. They cover single-process correctness for the OWL 2 EL profile against the default Oxigraph storage backend; they do not support distributed execution, do not persist intermediate reasoner state across process restarts, and advertise `persistent: false, distributed: false, max_axioms: Some(<a single-process figure>)` per RFC 0003's `ReasonerCapabilities`.

- **Premium impls** live in `semanticore-context-engine-{enduring,major,minor}` and drop in via `ReasonerSelection::Explicit(ReasonerLineup { enduring: <premium>, ... })`. They implement the same `ContextReasoner` trait, advertise `persistent: true, distributed: true` where applicable, and lift the `max_axioms` ceiling to whatever the underlying storage can hold. The trait surface is the contract; APIs above this line never change when swapping.

The Session's auto-selection algorithm — invoked when `ReasonerSelection::Auto` is configured — implements the contract that RFC 0003 deferred to this RFC. The algorithm runs once at `Session::open` and produces the lineup that the Session uses for the rest of its lifetime:

1. Enumerate every `ContextReasoner` impl linked into the binary (registered via inventory at compile time, or via `Session::register_reasoner` at runtime).
2. For each scope (`Enduring`, `Major`, `Minor`), select the reasoner whose `scope()` matches.
3. Among multiple matches per scope, score by capability:
   - Prefer `persistent: true` for the `Enduring` scope.
   - Prefer `distributed: true` if the SessionConfig advertises a multi-process deployment.
   - Prefer the higher `max_axioms` bound (or unbounded) if the configured `ScopeBounds` exceeds the OSS default's ceiling.
   - Among reasoners that tie on capability, pick by deterministic tiebreaker: the crate name, alphabetically (premium crates begin with `semanticore-context-engine-`, OSS defaults are inside `semanticore-context`, so premium wins when tied — by design).
4. If no reasoner matches a scope, the Session uses the OSS default, which is always linked.
5. If the resulting lineup violates the SessionConfig bounds (e.g., an OSS default whose `max_axioms` is below the configured `enduring_max_items`), `Session::open` returns `SessionError::CapabilityShortfall { scope, required, available }` rather than silently degrading.

The selection is per-Session, not per-call. Once chosen, the lineup is stable for the Session's lifetime; this is a deliberate departure from RFC 0003's per-call selection for OWL/SHACL/Datalog reasoners, because Context reasoners hold per-scope state that is expensive to reconstruct and switching mid-Session would require state transfer that is not yet specified.

The `Session::reason(scope)` call dispatches to the registered reasoner for that scope and invokes whichever of `Materialization::materialize`, `IncrementalReasoner::on_insert` / `on_remove`, or `Entailment::entailed_quads` is appropriate for the call site. Applications that want explicit control over which trait method is called drop down to the reasoner instance directly via `Session::reasoner(scope) -> &dyn ContextReasoner`.

## Storage integration

Storage is the substrate (RFC 0002). Each scope can use a different `Storage` backend in principle: a typical premium deployment puts the enduring tier on a persistent file-backed Oxigraph store, the major tier on an in-memory Oxigraph store, and the minor tier on a transient working buffer that lives only for the query.

v0.1 ships a single-storage shorthand: one `Storage` instance is shared across all three scopes, with the scope discriminator carried as graph metadata on each quad. This keeps the v0.1 implementation simple and matches the dominant deployment mode (a single Oxigraph store in-process). Multi-storage configuration — one `Storage` per scope — is a v0.2 capability and is governed by an extension to `StorageConfig`:

```rust
pub enum StorageConfig {
    /// v0.1: one Storage shared across all three scopes.
    Shared(Box<dyn Storage>),
    /// v0.2: one Storage per scope.
    PerScope {
        enduring: Box<dyn Storage>,
        major: Box<dyn Storage>,
        minor: Box<dyn Storage>,
    },
}
```

Whichever shape is configured, the Session uses the storage's `StorageCapabilities` to validate the configuration at `open` time. If a scope's configured storage does not advertise `persistent=true` and the scope is `Enduring`, `Session::open` returns `SessionError::CapabilityShortfall`. If the storage does not advertise `sparql_endpoint=true`, SPARQL queries fall back to the Session's pattern evaluator on top of `quads_for_pattern` (RFC 0002).

Transactions (RFC 0002) are used for every multi-quad operation. `Session::add_batch` opens a single write transaction, inserts every quad in the batch, and commits; `Session::query` opens a read transaction, runs the query, and rolls back. The Session is responsible for transaction lifetime; applications never see `StorageTransaction` directly.

## Bindings story

Both Python and TypeScript bindings expose the full Session API. The shape is identical across the two languages; only the async/sync convention differs.

### Python (PyO3)

```python
from semanticore_context import Session, ContextScope, ContextItem, SessionConfig

config = SessionConfig.in_memory()
with Session.open(config) as session:
    item = ContextItem.from_triples([
        ("urn:alice", "urn:knows", "urn:bob"),
    ])
    item_id = session.add(ContextScope.MAJOR, item)

    result = session.query(
        ContextQuery.pattern("urn:alice ? ?"),
    )
    for ranked in result.items:
        print(ranked.score, ranked.item)
```

The Python surface is **synchronous**. PyO3 bindings call into Rust on the calling thread; Python's GIL is released during the call, so concurrent Python threads can issue work to the same Session in parallel without serializing on the GIL. The context-manager protocol (`with Session.open(...) as session:`) wraps `open` and `close`; exiting the block always closes the Session, even on exception, which is the canonical way to guarantee enduring-tier flushes happen.

### TypeScript (napi-rs)

```typescript
import { Session, ContextScope, ContextItem, SessionConfig } from "@semanticore/context";

const config = SessionConfig.inMemory();
const session = await Session.open(config);
try {
    const item = ContextItem.fromTriples([
        ["urn:alice", "urn:knows", "urn:bob"],
    ]);
    const itemId = await session.add(ContextScope.Major, item);

    const result = await session.query(
        ContextQuery.pattern("urn:alice ? ?"),
    );
    for (const ranked of result.items) {
        console.log(ranked.score, ranked.item);
    }
} finally {
    await session.close();
}
```

The TypeScript surface is **asynchronous**. napi-rs runs Rust work on a worker thread pool (Tokio under the hood); every method returns a Promise. This matches Node-ecosystem expectations: blocking the event loop on a multi-second reasoning call is unacceptable, so the binding off-loads to a worker by default.

Native session lifetime is owned by the binding wrapper. In Python, the `Session` Python object holds an `Arc<Mutex<NativeSession>>`; the wrapper drops the native session in `__del__`, with `close` as the explicit early-release path. In TypeScript, the `Session` JavaScript object holds a Tokio task handle; `close()` is the only correct way to release the native session, and the binding emits a runtime warning if a Session is garbage-collected without `close()` having been called.

Both bindings re-export every public type from the Rust crate. There are no binding-only types and no binding-only behavior — a behavior gap between the bindings and the Rust crate is a bug. Round-trip parity tests at T3.5 enforce this.

## OSS at v0.1: what reasoners ship

Per the EC/MC/mC architecture decision logged in the rebuild plan and per the lessons carried forward from RFC 0003's "Capability advertisement and selection" section, the v0.1 OSS reasoner scope is locked down explicitly:

- `semanticore-context` v0.1 ships **default OSS reasoners only** — single-process, correctness-focused, no distributed support, no cross-process persistence of reasoner state.

- **OWL coverage is OWL 2 EL only.** The OSS default reasoner is an ELK-class implementation of OWL 2 EL, providing classification, realization, and consistency checking on bio-medical-scale ontologies within a single process. **OWL QL, OWL RL, and OWL DL are premium-only** at v0.1; calling `Session::reason(scope)` with a scope whose entailments require QL / RL / DL behavior succeeds against the EL subset only, and any axiom outside EL is reported via `OwlError::UnsupportedAxiom { axiom, profile }` per RFC 0003.

- **SHACL is premium-only at v0.1.** The Session exposes the SHACL surface (`Session::validate(data, shapes)` is reserved in the API), but at v0.1 the default OSS reasoner returns `ContextError::ShaclUnavailable` rather than running validation. Applications that need SHACL link `semanticore-reasoner-shacl` from the premium tier.

- **Datalog is premium-only at v0.1.** Same pattern as SHACL: the Session reserves the Datalog surface, but the default OSS reasoner refuses any Datalog operation with `ContextError::DatalogUnavailable`. Applications that need Datalog link `semanticore-reasoner-datalog` from the premium tier.

The Session's auto-selection accordingly: if no premium reasoner is linked, the OSS default reasoner is selected for all three scopes for OWL EL operations, and the SHACL / Datalog surfaces refuse with a clear error rather than silently no-opping. This is a deliberate design choice — silent no-ops were the single largest source of correctness drift in the legacy SDK, and the v0.1 API treats unavailable functionality as a hard error so applications either link the premium tier or change their code to not call the unavailable surface.

The full OSS-tier capability advertisement at v0.1:

| Capability | Enduring | Major | Minor |
|---|---|---|---|
| `persistent` | true (file-backed Oxigraph) | false (in-memory) | false (transient) |
| `distributed` | false | false | false |
| `incremental` | true (EL) | true (EL) | true (EL) |
| `explanation` | false | false | false |
| `max_axioms` | 1M | 100K | 10K |

The premium tier's capability advertisement is documented in RFC 0006 (premium boundary) in the private repo.

## Migration notes

Any prototype previously associated with this surface area is being replaced wholesale. The legacy Python-first SDK at `~/Desktop/development/semantic-core/semanticore` had a Session-like type, but its scopes, reasoner integration, query DSL, and lifecycle were all shaped around Python idioms and against an ad-hoc graph wrapper that no longer exists. No compatibility shim will be provided between the legacy surface and this RFC's surface. The public API specified here is greenfield: callers migrate by rewriting against the new types. Internal historical references to the predecessor codename appear only in this RFC's introduction and in RFC 0001's codename map; they will not appear anywhere in the source, the documentation, or the bindings.

## Open questions

- **Federation across Sessions on different machines.** A multi-machine working set (one Major tier shared across a fleet of Sessions) is a premium-tier concern and is implemented in `semanticore-context-engine-major`. v0.1 OSS does not federate; v0.2 OSS may add a thin federation hook driven by user demand. Tracked in the premium boundary RFC.

- **Streaming reasoning.** Sub-second incremental entailment over very large enduring graphs is a premium concern (the `enduring` engine is the candidate venue). The base `IncrementalReasoner` trait from RFC 0003 already supports the operation; what is deferred is the latency and throughput contract.

- **Embedding-store interoperability.** v0.1 ships a default ANN index inside `semanticore-context` (a small HNSW implementation suitable for single-process workloads up to ~1M chunks). Production deployments routinely outgrow that ceiling and want to plug in pgvector, Qdrant, or Pinecone. The v0.1 surface does not expose a swap-in point for the ANN index; v0.2 will add a `VectorStore` trait analogous to the `Storage` trait, with the default HNSW becoming one impl among many. This is the highest-priority post-1.0 trait extension.

- **Schema introspection.** `Session::describe()` — return the set of class IRIs, property IRIs, and per-property statistics — is a useful read-only operation that several agentic application patterns ask for (showing the user what the agent knows, generating dynamic UI from the schema). It is not in v0.1; deferred pending design.

- **Multi-tenant access control inside a Session.** Splitting a Session's tiers by tenant (per-user enduring tier, shared major tier, isolated minor tier per request) is a premium MC engine concern. The v0.1 OSS Session is single-tenant by construction; the premium engines add tenant discriminators inline with their distributed implementation. Not in scope here; tracked in the premium boundary RFC.

- **Background reasoning ticks.** Continuous materialization (the Session re-runs `reason(scope)` automatically on a configurable cadence) is a v0.2 capability. v0.1 is on-demand only; applications that want background reasoning can run their own ticker that calls `Session::reason` on a schedule.

- **Cross-Session item identity.** ItemIds are stable within a Session but not across Session restarts. Applications that need cross-restart identity use the item's IRI (for `Triples` payloads) or a content hash (for `Chunk` and `Event` payloads) and resolve to a fresh ItemId after restart. v0.2 may add a stable cross-Session id scheme; the v0.1 position is that cross-Session identity is the application's concern.
