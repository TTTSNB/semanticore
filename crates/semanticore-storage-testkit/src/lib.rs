//! Dev-only test kit for `semanticore-core`'s `Storage` trait (RFC 0002).
//!
//! Not published. It holds:
//!
//! - [`contract`]: the storage contract as functions generic over
//!   `S: Storage`, each taking a fresh-store factory;
//! - [`storage_contract_suite!`]: a macro that turns the whole contract into
//!   `#[test]` functions for one backend, so every backend runs the same suite;
//! - [`strategies`]: proptest strategies for quads;
//! - [`MemStore`]: a reference in-memory store, the contract's second
//!   implementation;
//! - [`WithGuard`]: keeps e.g. a temporary directory alive as long as the
//!   store that lives in it.
//!
//! Add it as a dev-dependency:
//!
//! ```toml
//! [dev-dependencies]
//! semanticore-storage-testkit = { path = "../semanticore-storage-testkit" }
//! ```
//!
//! and in an integration test:
//!
//! ```ignore
//! semanticore_storage_testkit::storage_contract_suite!(mod mem_store, cases = 256, fresh = || MemStore::new());
//! ```

pub mod contract;
mod guard;
mod mem;
pub mod strategies;

pub use guard::WithGuard;
pub use mem::{MemStore, MemTx};
#[doc(hidden)]
pub use proptest;

use semanticore_core::model::{NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term};

/// Reference semantics for a quad pattern: `None` matches anything.
pub fn matches_pattern(
    q: &Quad,
    s: Option<&NamedOrBlankNode>,
    p: Option<&NamedNode>,
    o: Option<&Term>,
    g: Option<&NamedOrDefault>,
) -> bool {
    s.is_none_or(|s| &q.subject == s)
        && p.is_none_or(|p| &q.predicate == p)
        && o.is_none_or(|o| &q.object == o)
        && g.is_none_or(|g| &q.graph == g)
}

/// Generate the full storage contract as `#[test]` functions in a module.
///
/// - `mod <name>`: the module the tests go in (so one test binary can run
///   the suite for several backends);
/// - `cases = <u32>`: proptest cases per property;
/// - `fresh = <expr>`: an expression of type `impl Fn() -> S` where
///   `S: Storage`, called once per test case for a new, empty store. It is
///   expanded inside the module, which glob-imports its parent (`use
///   super::*`), so it can name items from the calling file.
#[macro_export]
macro_rules! storage_contract_suite {
    (mod $name:ident, cases = $cases:expr, fresh = $fresh:expr $(,)?) => {
        mod $name {
            #[allow(unused_imports)]
            use super::*;
            use $crate::contract;
            use $crate::proptest::prelude::*;
            use $crate::strategies::*;

            $crate::proptest::proptest! {
                #![proptest_config(ProptestConfig::with_cases($cases))]

                #[test]
                fn round_trip(quads in small_quads(24)) {
                    contract::round_trip($fresh, &quads);
                }

                #[test]
                fn idempotent_insert(quads in small_quads(24)) {
                    contract::idempotent_insert($fresh, &quads);
                }

                #[test]
                fn delete_after_insert(base in small_quads(16), extra in small_quad()) {
                    contract::delete_after_insert($fresh, &base, &extra);
                }

                #[test]
                fn pattern_matches_reference(
                    quads in small_quads(24),
                    s in maybe(small_subject()),
                    p in maybe(small_named()),
                    o in maybe(small_term()),
                    g in maybe(small_graph()),
                ) {
                    contract::pattern($fresh, &quads, s.as_ref(), p.as_ref(), o.as_ref(), g.as_ref());
                }

                #[test]
                fn pattern_from_member_finds_it(
                    quads in small_quads(16).prop_filter("non-empty", |v| !v.is_empty()),
                    idx in any::<prop::sample::Index>(),
                    mask in any::<[bool; 4]>(),
                ) {
                    let q = idx.get(&quads).clone();
                    contract::pattern_from_member($fresh, &quads, &q, mask);
                }

                #[test]
                fn rollback_and_isolation(base in small_quads(12), writes in small_quads(12)) {
                    contract::rollback_and_isolation($fresh, &base, &writes);
                }

                #[test]
                fn drop_discards_writes(base in small_quads(12), writes in small_quads(12)) {
                    contract::drop_discards_writes($fresh, &base, &writes);
                }

                #[test]
                fn read_only_rejects_writes(base in small_quads(12), q in small_quad()) {
                    contract::read_only_rejects_writes($fresh, &base, &q);
                }

                #[test]
                fn named_graphs_are_distinct_and_complete(quads in small_quads(24)) {
                    contract::named_graphs($fresh, &quads);
                }

                #[test]
                fn named_graphs_track_removals(
                    quads in small_quads(24),
                    remove in prop::collection::vec(any::<bool>(), 1..8),
                ) {
                    contract::named_graphs_track_removals($fresh, &quads, &remove);
                }

                #[test]
                fn snapshot_reads(base in small_quads(12), writes in small_quads(12)) {
                    contract::snapshot_reads($fresh, &base, &writes);
                }
            }

            #[test]
            fn empty_store_is_empty() {
                contract::empty_store_is_empty($fresh);
            }

            #[test]
            fn graphs_are_separate() {
                contract::graphs_are_separate($fresh);
            }

            #[test]
            fn writes_after_rollback() {
                contract::writes_after_rollback($fresh);
            }

            #[test]
            fn shareable_across_threads() {
                contract::shareable_across_threads($fresh);
            }
        }
    };
}
