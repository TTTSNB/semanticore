# RFC 0003: Reasoner trait surface

- **Status:** Accepted
- **Date:** 2026-05-07
- **Author:** Elliott Risch
- **Phase:** 2 — Architecture spec
- **Depends on:** RFC 0001
- **Related:** RFC 0002 (Storage)
- **Crates affected:** `semanticore-core` defines all traits in this RFC; OSS default impls live in `semanticore-context`; premium impls live across `semanticore-reasoner-*` and `semanticore-context-engine-*`.

## Summary

This RFC defines the canonical reasoner trait surface for semantiCore. A small set of base traits (`Reasoner`, `Materialization`, `IncrementalReasoner`, `Entailment`) factor out the inference patterns common to every reasoning family, and four specialized traits (`OwlReasoner`, `ShaclValidator`, `DatalogEngine`, `ContextReasoner`) capture the family-specific surfaces that consumers actually call. All traits live in OSS (`semanticore-core`) so the public API of semantiCore is complete without any premium crate; premium crates (the six `semanticore-reasoner-*` and three `semanticore-context-engine-*` crates from RFC 0001) provide drop-in implementations that supply distributed, persistent, and high-throughput behavior without altering the trait surface itself.

## Motivation

The two-tier architecture established in RFC 0001 lives or dies on the discipline of one rule: **OSS defines the traits, premium implements them**. The reasoner traits are the load-bearing case for that rule. A user who installs only the OSS tier must get a working semantiCore — single-process, correct, embedded — driven by default reasoners that ship inside `semanticore-context`. A user who later adds the premium tier must transition by swapping a Cargo dependency, not by rewriting application code. Both experiences depend on a trait surface that is identical in shape across OSS defaults and premium implementations.

The reasoner approach decision (resolved in Phase 0: greenfield Rust per published research) shapes what these traits expose. Rather than wrap an existing reasoner, semantiCore implements every family from scratch: ELK / Whelk-class for OWL EL, tableaux-class for OWL DL, trav-SHACL-class for SHACL, semi-naive evaluation with stratified negation for Datalog, and a fresh design for the three Context-engine scopes (`enduring`, `major`, `minor`). The benchmark target is the union of Treehouse, RDFox, Stardog, GraphDB, and AllegroGraph, so the traits must be expressive enough to support optimizations those reasoners exploit (incremental update propagation, query-time entailment for read-mostly workloads, capability-driven planner hints) without forcing any particular implementation strategy.

The OSS-scope decision also shapes the surface. OSS reasoners are scoped to "single-process, correct, reasonably fast" — which means OSS implementations may panic on inputs larger than memory, may be single-threaded, and may not persist intermediate state across process restarts. The trait surface accommodates this by separating capability advertisement (`ReasonerCapabilities`) from the operations themselves: an OSS reasoner advertises `persistent: false, distributed: false`; a premium reasoner advertises `persistent: true, distributed: true`. The `Session` in `semanticore-context` selects among linked reasoners using these capabilities, so an application that needs distributed reasoning compiles only when a distributed implementation is linked.

Finally, the trait surface must serve the Context OS first. RFC 0001 names `semanticore-context` as the OSS Context-OS crate, and the three `semanticore-context-engine-*` premium crates implement its reasoning trait at scale. `ContextReasoner` is therefore a peer trait of `OwlReasoner` / `ShaclValidator` / `DatalogEngine`, not a wrapper or a higher-level abstraction. The same base-trait fundamentals apply uniformly across all four families.

## Common reasoner fundamentals

Every reasoner in semantiCore implements the `Reasoner` base trait. Most reasoners also implement one or more of `Materialization`, `IncrementalReasoner`, and `Entailment`, depending on what the family supports.

### `Reasoner` (base trait)

```rust
pub trait Reasoner: Send + Sync {
    type Storage: crate::storage::Storage;
    type Error: std::error::Error + Send + Sync + 'static;

    /// Capabilities this reasoner advertises. Used by the Session to pick
    /// among linked reasoners at runtime.
    fn capabilities(&self) -> ReasonerCapabilities;

    /// Bind this reasoner to a storage instance, producing a view that
    /// scopes any subsequent operation to that storage.
    fn over(&self, storage: &Self::Storage) -> ReasonerView<'_, Self>
    where
        Self: Sized;
}
```

The `Storage` associated type ties a reasoner to a concrete backend (RFC 0002), so the compiler enforces compatibility: an `OwlElReasoner<OxigraphStorage>` cannot be passed an `S3Storage` instance by mistake. `Send + Sync` is mandatory because the Context OS calls reasoners from async tasks.

### `Materialization`

A reasoner that computes the entailed graph and writes it back to storage. This is the dominant pattern for OWL EL/QL/RL and Datalog.

```rust
pub trait Materialization: Reasoner {
    /// Compute all entailments and persist them to storage. Returns
    /// statistics on what was added.
    fn materialize(&self, storage: &mut Self::Storage)
        -> Result<MaterializationStats, Self::Error>;

    /// Remove all materialized entailments, leaving the asserted base.
    /// Implementations that tag derived quads with provenance use that
    /// tag to identify what to remove.
    fn dematerialize(&self, storage: &mut Self::Storage)
        -> Result<(), Self::Error>;
}
```

`MaterializationStats` carries the count of quads added, the count of quads removed (rare; a few rule fires can retract), the wall-clock time, and the count of fixpoint iterations. It is the primary observability channel for materialization-heavy workloads.

### `IncrementalReasoner`

A reasoner that maintains the entailed graph under updates without redoing the full materialization. Counting algorithms (DRed, counting deletion) and overlay-graph approaches both fit this trait.

```rust
pub trait IncrementalReasoner: Materialization {
    /// Update materialization to reflect newly inserted quads. Caller
    /// must have already inserted them into storage.
    fn on_insert(&self, storage: &mut Self::Storage, inserted: &[Quad])
        -> Result<MaterializationStats, Self::Error>;

    /// Update materialization to reflect removed quads. Caller must have
    /// already removed them from the asserted base. Implementations may
    /// retract derived quads that no longer have a derivation.
    fn on_remove(&self, storage: &mut Self::Storage, removed: &[Quad])
        -> Result<MaterializationStats, Self::Error>;
}
```

`IncrementalReasoner` extends `Materialization` rather than standing alongside it because incremental update implies a baseline materialization to update against. A reasoner that supports incremental update can always re-materialize from scratch by calling `dematerialize` followed by `materialize`.

### `Entailment`

A reasoner that answers entailment queries without writing anything to storage. This is how OWL QL is typically used (query rewriting against the asserted base) and how OWL DL handles consistency / classification queries that don't justify a full materialization pass.

```rust
pub trait Entailment: Reasoner {
    /// Does the asserted graph entail the given quad?
    fn entails(&self, storage: &Self::Storage, candidate: &Quad)
        -> Result<bool, Self::Error>;

    /// Iterate over all entailed quads. Implementations may stream
    /// results lazily; callers that materialize the iterator into a
    /// collection accept the memory cost.
    fn entailed_quads<'a>(&'a self, storage: &'a Self::Storage)
        -> Box<dyn Iterator<Item = Result<Quad, Self::Error>> + 'a>;
}
```

A reasoner may implement both `Materialization` and `Entailment`; the choice between them at call time is the application's. The `entailed_quads` iterator is `Box<dyn ...>` rather than a generic associated type to keep the trait object-safe (a `Session` stores reasoners as `dyn Entailment`).

### `ReasonerCapabilities`

What an implementation declares about itself. The `Session` in `semanticore-context` reads this struct to pick among linked reasoners at runtime.

```rust
pub struct ReasonerCapabilities {
    /// OWL profiles supported. Empty for non-OWL reasoners.
    pub profiles: ProfileSet,

    /// Reasoner supports `IncrementalReasoner`.
    pub incremental: bool,

    /// Reasoner can spill state to disk and resume across process
    /// restarts. OSS defaults are `false`; premium impls are `true`.
    pub persistent: bool,

    /// Reasoner can run across processes or machines. OSS defaults are
    /// `false`; premium engines are `true`.
    pub distributed: bool,

    /// Reasoner can return a proof / explanation trace for an entailment.
    /// Deferred to v1.1; OSS and premium both default `false` at v0.1.
    pub explanation: bool,

    /// Hard upper bound on input axioms, if any. OSS defaults publish a
    /// figure consistent with the "single-process, in-memory" scope.
    pub max_axioms: Option<u64>,
}
```

`ProfileSet` is a small bitfield wrapping `OwlProfile` (defined below). Capabilities are inert data — the trait does not enforce them. Misadvertising is a contract violation that the Session detects only when an operation panics or returns an out-of-scope error.

## Specialized traits

### `OwlReasoner`

The OWL surface is unified across the four profiles via the `OwlProfile` enum. A single Rust type may implement multiple profiles when its algorithm subsumes them — for example, a Datalog-based RL implementation can also handle the EL-restricted subset, advertising both via `supported_profiles`.

```rust
pub enum OwlProfile {
    /// EL: subsumption / classification on bio-medical scale ontologies.
    El,
    /// QL: query rewriting over relational data.
    Ql,
    /// RL: rule-based; expressible in Datalog.
    Rl,
    /// DL: full OWL 2 DL via tableaux.
    Dl,
}

pub trait OwlReasoner: Materialization + IncrementalReasoner + Entailment {
    /// Profiles this reasoner can be asked to operate under.
    fn supported_profiles(&self) -> &[OwlProfile];

    /// Compute the class hierarchy (subClassOf closure with equivalence
    /// classes collapsed).
    fn classify(&self, storage: &Self::Storage)
        -> Result<ClassHierarchy, Self::Error>;

    /// Compute, for each named individual, its asserted and derived
    /// types.
    fn realize(&self, storage: &Self::Storage)
        -> Result<InstanceRealization, Self::Error>;

    /// Check global consistency. Returns `Ok(Consistent)` or
    /// `Ok(Inconsistent { explanation })` when explanation is supported.
    fn check_consistency(&self, storage: &Self::Storage)
        -> Result<ConsistencyResult, Self::Error>;
}
```

`ClassHierarchy` is a directed acyclic graph keyed by class IRI with `equivalent`, `direct_super`, and `direct_sub` accessors; it preserves equivalence classes rather than picking a canonical representative, so consumers can decide whether to canonicalize. `InstanceRealization` is a map from individual IRI to the set of derived class IRIs, paired with the witnessing axiom index when explanation is supported. Full type definitions live in `semanticore-core::owl`.

`OwlReasoner` extends all three of `Materialization`, `IncrementalReasoner`, and `Entailment` — every OWL reasoner in scope for v0.1 is expected to support all three patterns. (DL reasoners that do not natively support incremental update fall back to re-materialization in `on_insert` / `on_remove`; this is allowed and does not violate the trait.)

### `ShaclValidator`

```rust
pub trait ShaclValidator: Reasoner {
    /// Validate the data graph against the shapes graph. Returns a
    /// validation report that follows the SHACL Core conformance rules.
    fn validate(
        &self,
        data: &Self::Storage,
        shapes: &Self::Storage,
    ) -> Result<ShaclValidationReport, Self::Error>;

    /// Validate a single focus node. Used by interactive validation
    /// surfaces and by the Context OS for incremental constraint
    /// checking.
    fn validate_node(
        &self,
        data: &Self::Storage,
        shapes: &Self::Storage,
        focus: &NamedOrBlankNode,
    ) -> Result<NodeValidationResult, Self::Error>;

    /// Whether this implementation accepts `sh:sparql` constraints.
    /// SHACL Core is required; SHACL-SPARQL is the v0.1 stretch goal.
    fn supports_sparql_constraints(&self) -> bool;
}
```

SHACL Core plus SHACL-SPARQL is the v0.1 scope. SHACL Advanced (rules) is deferred — see Open questions. `ShaclValidator` is intentionally not a `Materialization` or `Entailment` trait: validation produces a report, not a graph delta. A future SHACL-rules trait would extend `Materialization` instead.

`data` and `shapes` are separate `Storage` references because real-world deployments often have data and shapes in different stores (different access controls, different update cadences). Implementations that prefer a unified store can implement `validate` by overlaying the two stores into a temporary view.

### `DatalogEngine`

```rust
pub trait DatalogEngine: Reasoner {
    /// Load a Datalog program. Replaces any previously loaded program.
    fn load_program(&mut self, program: &DatalogProgram)
        -> Result<(), Self::Error>;

    /// Evaluate the loaded program against an extensional database.
    /// Returns the intensional result.
    fn evaluate(&self, edb: &Self::Storage)
        -> Result<DatalogResult, Self::Error>;

    /// What kind of negation, if any, this engine supports.
    fn supports_negation(&self) -> NegationKind;

    /// Whether this engine supports aggregate atoms (count, sum, min,
    /// max, group_by). Deferred to v1.1 by default.
    fn supports_aggregation(&self) -> bool;
}

pub enum NegationKind {
    /// No negation. Pure positive Datalog.
    None,
    /// Stratified negation (the v0.1 scope).
    Stratified,
    /// Well-founded semantics. Deferred.
    Wellfounded,
}
```

Stratified negation with semi-naive evaluation is the v0.1 scope. Well-founded semantics is deferred — most production Datalog workloads can be expressed under stratified negation, and the implementation cost of well-founded is materially higher. The `NegationKind` enum is forward-compatible: when well-founded is added, no caller code changes.

`DatalogEngine::load_program` takes `&mut self` because loading a program changes the engine's planner state; subsequent `evaluate` calls run against the loaded program. This is consistent with how RDFox, Stardog, and Soufflé expose their Datalog surfaces. The `Reasoner` trait does not require interior mutability, so concrete implementations choose between `&mut self` and an `Arc<Mutex<...>>` wrapper as appropriate for their threading model.

### `ContextReasoner`

The reasoning surface for the Context OS. Default OSS impls live in `semanticore-context`; premium distributed/persistent impls live in `semanticore-context-engine-{enduring,major,minor}`.

```rust
pub enum ContextScope {
    /// Long-lived, slowly-evolving context (the "enduring" engine —
    /// internally formerly EC). Persists across sessions.
    Enduring,
    /// Session-lived context (the "major" engine — internally formerly
    /// MC). Holds the bulk of an active session's working state.
    Major,
    /// Turn-lived context (the "minor" engine — internally formerly mC).
    /// Discarded between turns; used for transient inferences.
    Minor,
}

pub trait ContextReasoner: Reasoner {
    /// Which scope this reasoner is responsible for.
    fn scope(&self) -> ContextScope;

    /// Promote items from a lower-lifetime scope to a higher-lifetime
    /// scope (e.g., Minor → Major when an inference proves durable, or
    /// Major → Enduring when a session's learnings should persist).
    fn promote(
        &self,
        from: ContextScope,
        to: ContextScope,
        items: &[ContextItem],
    ) -> Result<PromotionStats, Self::Error>;

    /// Remove items matching the given criteria from this scope.
    fn evict(
        &self,
        scope: ContextScope,
        criteria: &EvictionCriteria,
    ) -> Result<u64, Self::Error>;

    /// Capture a snapshot of this reasoner's state. Used for
    /// observability and for cross-process state transfer in premium
    /// distributed engines.
    fn snapshot(&self) -> Result<ContextSnapshot, Self::Error>;
}
```

The three scopes form a strict lifetime hierarchy: `Minor` is shortest-lived (one agent turn), `Major` spans an active session, `Enduring` persists indefinitely. The `promote` operation is the explicit transition between scopes — it is not implicit, because the cost of promoting an item from `Minor` to `Enduring` may include validation, deduplication, or an external review step that the application must drive.

OSS defaults in `semanticore-context` provide a single-process, in-memory implementation of all three scopes, suitable for development, single-user agentic applications, and any deployment under the OSS-scope envelope. Premium engines (`semanticore-context-engine-enduring`, `-major`, `-minor`) implement the same trait but back the storage with a distributed key-value store, persist across process restarts, and support multi-tenant isolation. The OSS-to-premium transition is a Cargo-level change: drop the premium crate into the workspace, register its reasoner with the Session in place of the OSS default, and existing application code compiles unchanged.

## OSS / premium boundary

The load-bearing rule, restated from RFC 0001 because it governs every line of this RFC: **the `Storage` trait, all reasoner traits, and the Context OS surface live in OSS — premium crates implement them; never define them**. If a premium crate needs a new trait, the trait is added to `semanticore-core` first (and reviewed against the OSS license commitment) before any premium crate can use it.

Concretely, the OSS-to-premium transition for each reasoner family looks like this:

- **OWL EL / QL / RL / DL.** OSS users get a single OWL reasoner shipping inside `semanticore-context` that handles the OWL 2 EL profile via a small ELK-class implementation. Users who need QL, RL, or DL — or who need a faster EL reasoner at scale — add `semanticore-reasoner-owl-{ql,rl,dl,el}` from Cloudsmith. The premium crates implement `OwlReasoner`. The Session reads `capabilities()`, sees that the premium reasoner advertises a richer `profiles` set, and routes OWL operations to it. Application code (`session.classify(&storage)`) does not change.

- **SHACL.** OSS users get no SHACL validator at v0.1 — SHACL is premium-only. (Adding an OSS SHACL Core validator is on the post-1.0 roadmap, governed by demand.) Premium users add `semanticore-reasoner-shacl`, which implements `ShaclValidator`. The Session exposes `session.validate(&data, &shapes)` only when at least one `ShaclValidator` is registered.

- **Datalog.** Same pattern as SHACL. OSS provides no Datalog engine at v0.1. Premium users add `semanticore-reasoner-datalog`.

- **Context engines.** OSS users get default in-memory implementations of all three `ContextReasoner` scopes shipping inside `semanticore-context`. Premium users replace one or more of them with `semanticore-context-engine-{enduring,major,minor}` for distributed / persistent scale. This is the most common drop-in scenario, because the Context OS is the spine of any agentic application built on semantiCore.

A user who reads the public crates and then reads the premium crates should see the trait definitions exactly once, in the OSS crate. That is the audit signal that the boundary is sound.

## Capability advertisement and selection

Reasoners are chosen at runtime by the Session, defined fully in RFC 0004. The selection algorithm is straightforward:

1. The application opens a `Session` and configures it with one or more `Storage` backends (RFC 0002).
2. Linked reasoners — OSS defaults plus any premium crates the application has compiled in — register themselves with the Session at construction time. Each registration provides the reasoner instance and its `ReasonerCapabilities`.
3. When the application calls a Session method that requires reasoning (e.g., `session.classify(...)` or `session.validate(...)`), the Session matches the request against registered reasoners by capability:
   - For OWL: pick a reasoner whose `profiles` includes the requested profile. Among multiple matches, prefer `distributed: true` if the application has flagged the session as distributed.
   - For SHACL: pick the SHACL validator with `supports_sparql_constraints == true` if any constraint requires it; otherwise any registered validator.
   - For Datalog: pick the engine whose `supports_negation` is at least `Stratified` for any program containing negated atoms.
   - For Context: pick the `ContextReasoner` whose `scope()` matches the operation's scope, preferring `persistent: true` for `Enduring` operations.
4. If multiple reasoners tie on capabilities, the Session uses a deterministic tiebreaker (the crate name, alphabetically) so behavior is reproducible across runs.
5. If no reasoner matches, the Session returns a `NoSuitableReasoner` error rather than silently degrading.

The selection is per-call, not per-session. A single session may route OWL EL queries to the OSS default reasoner and OWL DL queries to a premium DL reasoner if both are registered. Applications that want pinned routing (e.g., always use the premium reasoner for performance reasons even on EL workloads) configure the Session with explicit reasoner preference at construction time; this hook is part of RFC 0004.

## Error model

Each reasoner family exposes a family-specific error type via the `Reasoner::Error` associated type. Common patterns:

- **`OwlError`** with variants `ConsistencyViolation { explanation }`, `UnsupportedAxiom { axiom, profile }`, `MaterializationFailure { reason }`, `IncrementalUpdateRejected { reason }`, `QuotaExceeded { kind, limit }`. `ConsistencyViolation` is the load-bearing variant: a reasoner that detects an inconsistency must return this rather than panicking, because the Context OS treats consistency violations as recoverable application errors (the application can choose to retract the offending update).

- **`ShaclError`** with variants `ConstraintViolation { focus, constraint, message }` (rare — most violations are reported in the validation report, not raised as errors), `UnsupportedConstraintComponent { component }`, `SparqlExecutionError { source }`, `QuotaExceeded { kind, limit }`. The validation report is the dominant return channel; errors are reserved for situations where the validator itself cannot proceed (a malformed shape graph, a SPARQL constraint that fails to parse).

- **`DatalogError`** with variants `GroundingFailure { rule, reason }`, `StratificationViolation { rule_set }`, `EvaluationLimitExceeded { iterations }`, `QuotaExceeded { kind, limit }`. Stratification violations are detected at `load_program` time and reject programs that mix recursive negation. Grounding failures are detected at `evaluate` time when a rule body cannot be matched.

- **`ContextError`** with variants `ScopeMismatch { expected, found }`, `PromotionDenied { reason }`, `EvictionConflict { item, reason }`, `SnapshotFailure { reason }`, `QuotaExceeded { kind, limit }`. Premium engines add network and persistence variants on top of these (network timeouts, persistent-store unavailability) wrapped via `#[from]` on the family error.

Every family error includes a `QuotaExceeded` variant. Premium reasoners enforce license-tier quotas via `semanticore-licensing::require_feature`; OSS defaults enforce the in-memory single-process bounds advertised in `ReasonerCapabilities::max_axioms`. A `QuotaExceeded` error is the application's signal to back off, retry on a different reasoner, or surface a billing-tier prompt to the user. The error variants are stable across the OSS and premium tiers — premium engines do not introduce new error shapes, only new internal causes.

## Open questions

- **Proof / explanation generation.** The `ReasonerCapabilities::explanation` flag is in the trait surface but no method requires implementations to produce traces at v0.1. A follow-up RFC (target: v1.1) will add `OwlReasoner::explain(quad) -> Proof` and analogous methods on the other families, taking guidance from the explanation surfaces of RDFox and Stardog.

- **Reasoner composition** (e.g., chaining OWL RL → Datalog so that RL-derived facts become EDB to a Datalog program). This is a premium concern — the OSS scope does not promise composition. A follow-up RFC, deferred to post-1.0, will define a `ReasonerPipeline` trait or a Session-level fluent API.

- **Rule-based extension hooks for SHACL** (SHACL Advanced rules). Deferred. A future `ShaclRuleEngine` trait extending `Materialization` is the planned shape; it is intentionally absent at v0.1 to keep the SHACL surface focused on validation.

- **Streaming entailment** (sub-second incremental entailment over very large graphs). This is a premium `ContextReasoner` concern — the `enduring` engine is the candidate venue. The base `IncrementalReasoner` trait already supports the operation; what is deferred is the latency and throughput contract.

- **Cross-reasoner consistency.** When two reasoners disagree about whether a quad is entailed (e.g., an OWL DL reasoner says yes, an RL reasoner says no because of profile differences), the Session returns whatever the routed reasoner returns. A future RFC may define a `ConsistencyArbiter` for applications that need a unified answer; the v0.1 position is that profile differences are real and the application chooses the profile.

## Implementation notes

- Every trait in this RFC will be defined in `semanticore-core::reasoner` in Phase 3 (T3.1). The OSS default impls in `semanticore-context` (T3.2) import from `semanticore-core` and use only the OSS scope.
- The Session selection algorithm is normative for RFC 0004, not for this RFC. RFC 0004 may refine the tiebreaker rules and the error variants returned when no reasoner matches.
- All trait method signatures shown here are intent-level. Final method names and parameter orders may be adjusted in T3.1 once the core types (`Quad`, `NamedOrBlankNode`, `ContextItem`, etc.) are concrete; any such adjustments will be reflected in this document's revision history rather than scattered across implementation PRs.
- The `Send + Sync` bound on `Reasoner` is non-negotiable. The Context OS holds reasoners as `Arc<dyn Reasoner<...>>` inside async tasks; relaxing the bound breaks the Session.
