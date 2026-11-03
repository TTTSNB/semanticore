# AGENTS.md

Orientation for coding agents and new contributors working in the open tier of
semantiCore (Apache-2.0, public). Read this first; it points to the
authoritative documents rather than repeating them.

## Open-tier boundary

- This repository holds open-tier code only. Never add premium code, premium
  design detail, license keys, secrets or tokens.
- Never put client names, customer names, person names (other than GitHub
  handles already in the repo) or internal project codenames in code, docs,
  commit messages, issues or PRs.
- Traits live here; premium crates implement them and never define them
  (ARCHITECTURE.md, "Premium boundary"). SHACL, Datalog and OWL QL/RL/DL
  reasoners are premium-only; this tier returns a premium-only or
  not-yet-implemented error for them rather than silently doing nothing.

## Crate map

| Crate | Status |
| --- | --- |
| `semanticore-core` | Shared term model plus the `Storage` and reasoner trait surface (RFCs 0002, 0003). Traits and result types only. 0.1.0, not yet published to crates.io. |
| `semanticore-store-oxigraph` | First `Storage` backend, over Oxigraph 0.5.13: `in_memory`, `open` (RocksDB), `open_read_only`; `oxigraph()` exposes SPARQL and bulk load. 0.1.0. |
| `semanticore-storage-testkit` | Dev-only (`publish = false`). The RFC 0002 storage contract as generic functions, the `storage_contract_suite!` macro, quad strategies, reference `MemStore`. Every backend calls the macro once. |
| `semanticore-context` | Context OS skeleton (RFC 0004 v0.1): `Session` with enduring/major/minor tiers, promotion, eviction, demotion. SHACL and Datalog are premium-only errors; OWL 2 EL reasoner, ANN/HNSW index and SPARQL queries return not-yet-implemented errors. |
| `semanticore-context-py` | PyO3 binding for `semanticore-context`, abi3 wheel for CPython >= 3.10, import name `semanticore_context`. `publish = false`; nothing is on PyPI. |
| `semanticore-projection` | Specified (RFC 0005), not started. |
| TypeScript (napi-rs) binding | Specified (RFC 0004), not started. |

## Build and test

Rust toolchain: stable, MSRV 1.88 (`rust-version` in the root `Cargo.toml`).

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo test --workspace --doc --all-features
```

For several worktrees, share one `CARGO_TARGET_DIR` outside them and keep
`CARGO_BUILD_JOBS` low (4); the Oxigraph backend compiles RocksDB, which is slow
and large.

`semanticore-context-py` links libpython for its Rust-side tests, so
`cargo test --workspace` needs a CPython >= 3.10 with a shared library. If the
test binary fails to start with a missing `libpython3.x` error, put its library
directory on the loader path, for example
`LD_LIBRARY_PATH="$(python3 -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR"))')"`
on Linux (`DYLD_LIBRARY_PATH` on macOS). CI does this with
`$pythonLocation/lib`.

Python binding (pytest and stub checks, details in
`crates/semanticore-context-py/README.md`):

```sh
python -m venv .venv && . .venv/bin/activate
pip install "maturin>=1.16,<2" pytest mypy
maturin develop -m crates/semanticore-context-py/Cargo.toml
pytest crates/semanticore-context-py/tests
```

## CI jobs (`.github/workflows/ci.yml`)

| Job | Enforces |
| --- | --- |
| `lint` | `cargo fmt --check`, clippy with `-D warnings` |
| `test` | `cargo test --all --all-features` on stable (needs the setup-python shared libpython) |
| `msrv (1.88)` | build and test on exactly Rust 1.88 |
| `docs` | `cargo doc --no-deps` with `-D warnings`, plus doctests |
| `publish dry-run` | `cargo publish --dry-run -p semanticore-core` |
| `cargo-deny` | advisories, licenses, bans, sources per `deny.toml` |
| `semver-checks` | informational until `semanticore-core` 0.1.0 is on crates.io; see CONTRIBUTING.md for making it blocking |
| `python (3.10, 3.13)` | maturin abi3 wheel (tag `cp310-abi3`), pytest, `.pyi` stub check |

All workflow actions are pinned to commit SHAs with the tag in a trailing
comment. CONTRIBUTING.md is the source of truth for the local commands.

## deny.toml exception policy

- Add an advisory `ignore` only after assessing it, with the exact RUSTSEC id and
  a reason naming the dependency path, why this workspace is or is not exposed,
  and the condition for removing it. Currently RUSTSEC-2026-0194 and -0195
  (quick-xml via oxigraph 0.5.13); drop them when oxigraph moves to
  quick-xml >= 0.41.
- Prefer `skip`/`skip-tree` over relaxing `multiple-versions = "deny"`, and pin
  the exact version (the `oxigraph@0.5.13` skip-tree) so the entry stops
  applying, and the duplicates resurface for review, on upgrade.
- Licenses: permissive allow-list only; copyleft is rejected by omission.
  Sources: crates.io only.

## Where things are specified

- `docs/rfcs/0001`-`0005`: crate graph, Storage trait, reasoner traits, Context
  API, Projection API. RFC 0006 (premium boundary) is in the private companion
  repo. RFCs are not edited by implementation PRs; ambiguities go in the PR body
  as open questions and then into issues.
- `ARCHITECTURE.md`: one-page overview and Phase 2 reconciliations.
- `CHANGELOG.md`: add an `Unreleased` line for every user-visible change.
- `CONTRIBUTING.md`: workflow, commit style and the local gate commands.

## Working conventions

- Tests first; failing tests are committed before the implementation. Property
  tests use proptest; storage behaviour is tested only through the testkit
  contract so all backends stay identical.
- `unsafe_code` is forbidden workspace-wide.
- Public API changes need a CHANGELOG entry; breaking changes before 1.0 bump the
  minor version.
- Backlog lives in GitHub issues (label `backlog`).
