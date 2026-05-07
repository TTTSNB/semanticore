# RFC 0001: Crate graph and workspace layout

- **Status:** Accepted
- **Date:** 2026-05-07
- **Author:** Elliott Risch
- **Phase:** 2 — Architecture spec
- **Discussion:** N/A (greenfield rebuild)

## Summary

This RFC establishes the canonical crate graph and workspace layout for the semantiCore project across two repositories: a public open-source workspace at `TTTSNB/semanticore` (Apache 2.0) and a private commercial workspace at `TTTSNB/semanticore-premium` (proprietary, Cloudsmith-distributed). It defines every crate that will exist in either repo at 1.0, fixes the dependency direction (one-way, OSS-to-premium), names the workspace topology, and codifies the public-naming policy that strips internal codenames in favor of capability-first names. Every subsequent architecture RFC (storage trait, reasoner traits, Context API, Projection API, premium-boundary contract) depends on the names and boundaries established here, so this document is the foundation for the rest of Phase 2.

## Motivation

semantiCore is being rebuilt as a Rust-core, two-tier library that mirrors the maplib / Treehouse pattern: an Apache 2.0 open-source core with a separately-distributed commercial tier that drops production-grade reasoner and Context-engine implementations into the same trait surfaces. This rebuild deliberately scraps the prior Python-first SDK, so we have a single chance to get the package boundaries right before any code is written that downstream consumers will depend on.

Two repositories are required, not one. The OSS tier needs to live somewhere that contributors and auditors can read, fork, and depend on through public registries (crates.io, PyPI, npm). The premium tier ships closed-source reasoners and distributed Context engines under a commercial license, hosted on Cloudsmith private indexes, with runtime license-key gating. Mixing both license regimes in a single repository — even via subdirectory licensing — proved fragile in earlier internal experiments: CI secret separation gets harder, license-by-directory is ambiguous to package scanners, and a clean spinout (sponsorship vs spinout vs JV is still being decided in the business plan) is materially harder to execute when public and private code share a `git log`.

A Cargo workspace per repo lets the many small crates share a single `Cargo.lock`, a single rustfmt/clippy config, and a single CI pipeline, while still allowing each crate to publish independently with its own SemVer and changelog. This is the same pattern the Rust ecosystem standardized on for any non-trivial library family (`tokio`, `axum`, `oxigraph`, etc.) and it scales to the seventeen crates this RFC enumerates.

The naming policy — capability-first, no internal codenames — solves three problems simultaneously. First, the existing internal names (Theseus, Metasemantics, KI1-Prometheus, Pegasus, Gavagai, Nucleus, Sentinel) carry no meaning to external consumers and burn the first impression on a story we don't want to tell. Second, capability names survive a corporate-structure pivot: if EK spins this out, takes a JV partner, or contributes it to a foundation, the crate names don't presume any particular sponsor. Third, capability names map cleanly to the directory layout, making the codebase self-documenting (`semanticore-reasoner-owl-el` is unambiguous in a way `theseus` is not).

## Crate inventory

| Crate | Repo | Tier | Purpose | Bindings | Status |
|---|---|---|---|---|---|
| `semanticore-core` | public | OSS | Shared types: IRIs, terms, triples, graphs, `Storage` trait, reasoner traits | Rust only (re-exported via bindings) | Placeholder (T1.1); full impl T3.1 |
| `semanticore-context` | public | OSS | Context OS: Session, Enduring/Major/minor Context surfaces, default reasoners | Rust + Python (PyO3) + TS (napi-rs) | Empty; specced in RFC 0004 |
| `semanticore-projection` | public | OSS | Bidirectional RDF↔LPG with round-trip metadata + interpretation profile registry | Rust + Python + TS | Empty; specced in RFC 0005 |
| `semanticore-context-py` | public | OSS | Python binding for `semanticore-context` | n/a | Empty; T3.3 |
| `semanticore-context-ts` | public | OSS | TypeScript binding for `semanticore-context` | n/a | Empty; T3.4 |
| `semanticore-projection-py` | public | OSS | Python binding for `semanticore-projection` | n/a | Empty; T3.6 |
| `semanticore-projection-ts` | public | OSS | TypeScript binding for `semanticore-projection` | n/a | Empty; T3.7 |
| `semanticore-licensing` | premium | Commercial | Runtime license-key verification | Rust only | Skeleton (T1.6); full impl T4.10 |
| `semanticore-reasoner-owl-el` | premium | Commercial | OWL 2 EL profile reasoner (greenfield Rust) | Rust only | T4.1 |
| `semanticore-reasoner-owl-ql` | premium | Commercial | OWL 2 QL profile reasoner | Rust only | T4.2 |
| `semanticore-reasoner-owl-rl` | premium | Commercial | OWL 2 RL profile reasoner | Rust only | T4.3 |
| `semanticore-reasoner-owl-dl` | premium | Commercial | OWL 2 DL profile reasoner | Rust only | T4.4 |
| `semanticore-reasoner-shacl` | premium | Commercial | SHACL Core + SHACL-SPARQL validation | Rust only | T4.5 |
| `semanticore-reasoner-datalog` | premium | Commercial | Stratified Datalog with semi-naive evaluation | Rust only | T4.6 |
| `semanticore-context-engine-enduring` | premium | Commercial | Distributed/persistent EC engine (drops in for default OSS impl in `semanticore-context`) | Rust only | T4.7 |
| `semanticore-context-engine-major` | premium | Commercial | Distributed MC engine | Rust only | T4.8 |
| `semanticore-context-engine-minor` | premium | Commercial | Distributed minor Context engine | Rust only | T4.9 |

That is seven OSS crates plus ten premium crates: seventeen crates total at 1.0. No additional crates are anticipated for the 1.0 release. Maplib feature parity (OTTR, RML, Polars/Arrow, embedded SPARQL), retrieval (`semanticore-retrieval`, formerly KI1-Prometheus), JVM bindings, and WASM bindings are explicitly post-1.0 and will land in successor RFCs.

## Codename mapping (historical reference)

Internal codenames will continue to appear in chat history, prior commits on the legacy `semantic-core` repo, internal wiki entries, and presentations Elliott has already given. This table is the canonical translation. PRs, design docs, and external artifacts must use the public name; the codename is acceptable only in private decision-log entries and internal historical commentary.

| Internal codename | Public crate / package |
|---|---|
| Theseus | `semanticore-context` |
| Metasemantics | `semanticore-projection` |
| KI1-Prometheus | `semanticore-retrieval` (deferred — not in 1.0) |
| EC reasoner | `semanticore-context-engine-enduring` (premium) |
| MC reasoner | `semanticore-context-engine-major` (premium) |
| mC reasoner | `semanticore-context-engine-minor` (premium) |
| Pegasus, Gavagai, Nucleus, Sentinel | (out of scope for 1.0) |

A separate concern, also resolved here: the public `README` may note that semantiCore has internal pedigree under prior names, but only as a one-line historical pointer. The crate names, module names, type names, error variants, and CLI flags must all be capability-first. No Theseus / Metasemantics / Prometheus typeshed leaks into the public API.

## Workspace layout

### Public repo

```
~/Desktop/development/semantic-core/semanticore/
├── Cargo.toml                        # workspace
├── crates/
│   ├── semanticore-core/
│   ├── semanticore-context/
│   ├── semanticore-projection/
│   └── bindings/
│       ├── semanticore-context-py/
│       ├── semanticore-context-ts/
│       ├── semanticore-projection-py/
│       └── semanticore-projection-ts/
├── docs/
│   ├── rfcs/
│   └── recipes/
├── examples/
└── ARCHITECTURE.md
```

The `crates/bindings/` subdirectory is a logical grouping only — Cargo treats every binding as a peer workspace member. Grouping the four bindings under one folder keeps the top-level `crates/` listing focused on the three Rust library crates a contributor is most likely to open first.

### Premium repo

```
~/Desktop/development/semantic-core/semanticore-premium/
├── Cargo.toml
├── crates/
│   ├── semanticore-licensing/
│   ├── semanticore-reasoner-owl-el/
│   ├── semanticore-reasoner-owl-ql/
│   ├── semanticore-reasoner-owl-rl/
│   ├── semanticore-reasoner-owl-dl/
│   ├── semanticore-reasoner-shacl/
│   ├── semanticore-reasoner-datalog/
│   ├── semanticore-context-engine-enduring/
│   ├── semanticore-context-engine-major/
│   └── semanticore-context-engine-minor/
├── docs/
│   ├── PREMIUM_BOUNDARY.md
│   └── registries.md
└── tests/integration/
```

Premium crates are flat under `crates/` because there are no premium bindings (premium crates are Rust-only — they implement traits defined in the OSS Context library, and OSS bindings re-export through the trait surface, so a Python or TypeScript user transparently picks up the premium reasoner once the OSS Context library is configured to use it).

## Dependency rules

These are hard rules, enforced by the workspace boundary itself (the two repos cannot path-depend on each other) and reinforced by review:

- **OSS crates must not depend on premium crates.** The dependency direction is one-way only. An OSS crate that "needs" a premium reasoner must instead define a trait that the premium crate implements.
- **Premium crates may depend on OSS crates and on each other.** Every premium crate will path-depend on `semanticore-core`, most will depend on `semanticore-context`, and any premium crate that wants license gating will depend on `semanticore-licensing`.
- **`semanticore-licensing` is the only premium crate that every other premium crate depends on directly.** It exposes `verify_key`, `require_feature`, and the `Feature` enum. Every premium reasoner and engine calls `require_feature(Feature::OwlEl)` (or its analogue) at construction time.
- **Bindings depend only on their corresponding library crate plus PyO3/napi-rs and serde.** A binding crate is intentionally thin: it must not contain logic, only marshaling. Logic lives in the Rust core where it is testable and reusable.
- **The `Storage` trait, all reasoner traits, and the Context OS surface live in OSS — premium crates implement them; never define them.** This is the load-bearing rule that lets the OSS crates ship a complete API that premium crates extend without modification. If a premium crate needs a new trait, the trait is added to the appropriate OSS crate first.

A consequence of these rules: a user with only the OSS crates installed gets a working semantiCore — single-process correct default reasoners, embedded Oxigraph storage, full Context and Projection APIs. Adding the premium tier swaps in distributed engines and OWL/SHACL/Datalog reasoners without changing any user-visible API.

## Versioning + release strategy

The default versioning posture is **workspace-shared**: both `Cargo.toml` files declare `[workspace.package] version = "x.y.z"` and individual crates use `version.workspace = true`. This keeps the seven OSS crates (and separately, the ten premium crates) moving in lockstep through 1.0, simplifying the release narrative ("semantiCore 0.1.0 is out" rather than "core 0.1.0, context 0.1.2, projection 0.0.9...").

Per-crate version overrides are allowed but require a documented reason in the crate's `CHANGELOG.md`. The expected reasons are:

- A crate has a known-bad release that needs a patch bump while everything else stays still.
- A binding crate has a build-system fix (PyO3 ABI bump, napi-rs platform matrix) that doesn't affect Rust.
- Post-1.0, a crate ages out at a different cadence (e.g., `semanticore-core` stays on a long-LTS rhythm while reasoners iterate faster).

SemVer is binding once 1.0 ships. Pre-1.0 (the entire scope of this rebuild through Phase 8), `0.x.y` increments may break API at minor-version boundaries with a clear `CHANGELOG.md` entry. The `0.1.0` release at the end of Phase 3 (OSS) and Phase 4 (premium) is the first version any external user is expected to consume; before that, the registries hold only `0.0.x` placeholders.

## Naming convention

Every public crate name follows one of four patterns. The pattern is mechanical — given a capability, there is exactly one correct crate name.

1. **`semanticore-<capability>`** — top-level OSS library crates: `semanticore-core`, `semanticore-context`, `semanticore-projection`.
2. **`semanticore-<capability>-<lang>`** — language bindings for an OSS library: `semanticore-context-py`, `semanticore-projection-ts`. The `<lang>` suffix is exactly `py` or `ts` (not `python` or `typescript`); it matches the file-extension idiom and keeps PyPI / npm package names short. PyPI and npm package names follow the same pattern, with PyPI using underscores (`semanticore_context_py`) and npm using a scoped namespace (`@semanticore/context`).
3. **`semanticore-reasoner-<family>`** — premium reasoner crates, where `<family>` is one of `owl-el`, `owl-ql`, `owl-rl`, `owl-dl`, `shacl`, `datalog`. New families added post-1.0 (e.g., `swrl`, `n3`) follow the same shape.
4. **`semanticore-context-engine-<scope>`** — premium Context-engine crates, where `<scope>` is one of `enduring`, `major`, `minor`. The "enduring / major / minor" terminology is the public-facing replacement for the internal "EC / MC / mC" abbreviations and is the first place those words appear in the public crate graph.

Three rules govern the naming convention itself:

- Capability-first: the crate name describes what the crate does, not its internal pedigree or its author.
- No codenames: any name from the codename map above is a review-block.
- ASCII lowercase with hyphens, no underscores in crate names (matches the dominant Rust ecosystem convention; underscores remain for module names within crates).

## Alternatives considered

**Single repo with public + premium subdirs under different licenses.** Rejected. The license boundary becomes a directory convention rather than a repo boundary, which scanners and downstream license-audit tools handle inconsistently. CI secret separation (publish tokens for crates.io vs Cloudsmith) becomes a per-job concern instead of a per-repo concern. A future spinout, JV, or sponsor-foundation contribution becomes materially harder when the two tiers share a `git log` and an issue tracker.

**Codename retention (Theseus / Metasemantics / KI1-Prometheus).** Rejected. The external-artifact naming rule (no internal codenames in public-facing materials, recorded in `feedback_no_internal_codenames.md`) applies just as strictly to crate names as it does to resumes and recruiter emails. Beyond the rule, codenames foreclose corporate-structure flexibility: `theseus` presupposes an EK pedigree the public crates may need to outgrow.

**Per-language workspaces (Rust + separate Python repo + separate TypeScript repo).** Rejected. Bindings inevitably drift from the core they wrap when they live in different repos with different release cadences. Keeping bindings as workspace members of the same repo as their library crate guarantees they compile against the same version of the library and ship together.

**No bindings (Rust-only library).** Rejected. The 1.0 audience is agentic-application engineers and data scientists, both of whom live primarily in Python. Shipping Rust-only would force every consumer to hand-roll bindings or wait for a community port. Python (PyO3) is non-negotiable for v1; TypeScript (napi-rs) is included because the agentic-application surface is split between Python and Node-based runtimes.

## Open questions

This RFC is intentionally narrow — it defines names and boundaries, not the contents of any crate. The following questions are deferred to later RFCs in this phase:

- Storage backend implementations beyond Oxigraph (file-based, distributed, cloud) → RFC 0002.
- Reasoner trait shapes (input/output types, error handling, incremental support) → RFC 0003.
- Context API public surface (Session, Enduring/Major/minor Context types, lifecycle) → RFC 0004.
- Projection API public surface (RDF↔LPG, round-trip metadata schema, interpretation profile registry) → RFC 0005.
- Premium-boundary mechanics (how OSS traits are versioned, how breakage is communicated to premium crates, integration-test contract) → RFC 0006 (lives in the premium repo).

## Implementation notes

- This RFC is **descriptive of the target state, not the current state**. Many crates listed do not exist yet. Phase 3 creates the OSS implementation crates and bindings; Phase 4 creates the premium reasoners and engines.
- Already created and committed: `semanticore-core` as a placeholder workspace member (T1.1), `semanticore-licensing` as a skeleton with real public API but a non-functional verifier (T1.6).
- Branch protection on the public repo requires `lint` and `test` checks to pass on PRs to `main`; this RFC merges direct-on-main per the Phase 2 RFC convention (markdown-only changes, no source touched).
- The directory layout shown for `crates/bindings/` is an aspiration — Cargo permits arbitrary nesting under `crates/` as long as workspace members enumerate the actual paths. The workspace `Cargo.toml` will be updated to reflect this layout when the corresponding bindings crates are scaffolded in Phase 3.
