//! Property tests for the RFC 0002 `Storage` / `StorageTransaction` contract.
//!
//! The crate ships no storage backend, so these run against the test-only
//! `MemStore` in `tests/common`. Each `contract_*` function is generic over
//! `S: Storage` and takes a fresh-store factory, so a real adapter can be
//! plugged in by calling the same functions with its own constructor.

mod common;

use std::collections::HashSet;

use common::*;
use proptest::prelude::*;
use semanticore_core::model::{NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term};
use semanticore_core::storage::{Storage, StorageError, StorageTransaction};

// ---------------------------------------------------------------------------
// Generic contract checks
// ---------------------------------------------------------------------------

/// Every quad currently visible to `tx`, via the all-wildcard pattern.
fn all_quads<T: StorageTransaction>(tx: &T) -> HashSet<Quad> {
    tx.quads_for_pattern(None, None, None, None)
        .map(|r| r.expect("pattern iteration failed"))
        .collect()
}

/// Committed state, read through a fresh read transaction.
fn committed<S: Storage>(store: &S) -> HashSet<Quad> {
    let tx = store.begin_read().unwrap();
    let out = all_quads(&tx);
    tx.rollback().unwrap();
    out
}

fn load<S: Storage>(store: &S, quads: &[Quad]) {
    let mut tx = store.begin_write().unwrap();
    for q in quads {
        tx.insert(q).unwrap();
    }
    tx.commit().unwrap();
}

/// Round trip: inserting quads and committing makes exactly that set
/// visible, through `contains`, the wildcard pattern, and `quad_count`.
fn contract_round_trip<S: Storage>(fresh: impl Fn() -> S, quads: &[Quad]) {
    let store = fresh();
    load(&store, quads);
    let expected: HashSet<Quad> = quads.iter().cloned().collect();

    let tx = store.begin_read().unwrap();
    for q in quads {
        assert!(tx.contains(q).unwrap(), "inserted quad missing: {q:?}");
    }
    assert_eq!(all_quads(&tx), expected);
    tx.rollback().unwrap();

    assert_eq!(store.quad_count().unwrap(), expected.len() as u64);
}

/// Idempotent insert: inserting the same quads again changes nothing, and
/// `insert` reports `true` exactly for the first occurrence of each quad.
fn contract_idempotent_insert<S: Storage>(fresh: impl Fn() -> S, quads: &[Quad]) {
    let store = fresh();
    let mut seen = HashSet::new();
    let mut tx = store.begin_write().unwrap();
    for q in quads {
        let changed = tx.insert(q).unwrap();
        assert_eq!(changed, seen.insert(q.clone()), "insert() return for {q:?}");
    }
    tx.commit().unwrap();
    let once = committed(&store);

    let mut tx = store.begin_write().unwrap();
    for q in quads {
        assert!(!tx.insert(q).unwrap(), "re-insert reported a change: {q:?}");
    }
    tx.commit().unwrap();
    assert_eq!(committed(&store), once);
    assert_eq!(store.quad_count().unwrap(), once.len() as u64);
}

/// Delete-after-insert: inserting a quad that is not yet present and then
/// removing it leaves the store exactly as before, whether the two steps
/// share a transaction or are committed separately.
fn contract_delete_after_insert<S: Storage>(fresh: impl Fn() -> S, base: &[Quad], extra: &Quad) {
    let store = fresh();
    load(&store, base);
    let before = committed(&store);
    let was_present = before.contains(extra);

    // Same transaction.
    let mut tx = store.begin_write().unwrap();
    assert_eq!(tx.insert(extra).unwrap(), !was_present);
    assert!(
        tx.remove(extra).unwrap(),
        "remove after insert must report a change"
    );
    assert!(!tx.contains(extra).unwrap());
    tx.commit().unwrap();

    let mut expected = before.clone();
    expected.remove(extra);
    assert_eq!(committed(&store), expected);

    // Restore if it was part of the base, then do it across two commits.
    load(&store, base);
    assert_eq!(committed(&store), before);
    if !was_present {
        load(&store, std::slice::from_ref(extra));
        let mut tx = store.begin_write().unwrap();
        assert!(tx.remove(extra).unwrap());
        tx.commit().unwrap();
        assert_eq!(committed(&store), before);
    }

    // Removing something absent reports no change and changes nothing.
    let mut tx = store.begin_write().unwrap();
    if !was_present {
        assert!(!tx.remove(extra).unwrap());
    }
    tx.rollback().unwrap();
    assert_eq!(committed(&store), before);
}

/// Pattern queries return exactly the quads matching the reference
/// semantics (`None` = wildcard).
fn contract_pattern<S: Storage>(
    fresh: impl Fn() -> S,
    quads: &[Quad],
    s: Option<&NamedOrBlankNode>,
    p: Option<&NamedNode>,
    o: Option<&Term>,
    g: Option<&NamedOrDefault>,
) {
    let store = fresh();
    load(&store, quads);
    let tx = store.begin_read().unwrap();
    let got: Vec<Quad> = tx
        .quads_for_pattern(s, p, o, g)
        .map(|r| r.unwrap())
        .collect();
    let got_set: HashSet<Quad> = got.iter().cloned().collect();
    assert_eq!(got.len(), got_set.len(), "pattern yielded duplicates");
    let expected: HashSet<Quad> = quads
        .iter()
        .filter(|q| matches_pattern(q, s, p, o, g))
        .cloned()
        .collect();
    assert_eq!(got_set, expected);
}

/// Rollback discards every write; uncommitted writes are visible inside the
/// writing transaction (read-your-writes) but not to a concurrent reader.
fn contract_rollback_and_isolation<S: Storage>(
    fresh: impl Fn() -> S,
    base: &[Quad],
    writes: &[Quad],
) {
    let store = fresh();
    load(&store, base);
    let before = committed(&store);

    let mut tx = store.begin_write().unwrap();
    for q in writes {
        tx.insert(q).unwrap();
        assert!(tx.contains(q).unwrap(), "read-your-writes violated");
    }
    if let Some(first) = base.first() {
        tx.remove(first).unwrap();
        assert!(
            !tx.contains(first).unwrap(),
            "read-your-writes violated (remove)"
        );
    }
    assert_eq!(committed(&store), before, "uncommitted writes leaked");
    tx.rollback().unwrap();

    assert_eq!(committed(&store), before);
}

/// Read-only transactions reject writes with an error (not a panic) and
/// leave the store untouched.
fn contract_read_only_rejects_writes<S: Storage>(fresh: impl Fn() -> S, base: &[Quad], q: &Quad) {
    let store = fresh();
    load(&store, base);
    let before = committed(&store);

    let mut tx = store.begin_read().unwrap();
    assert!(tx.insert(q).is_err());
    assert!(tx.remove(q).is_err());
    assert_eq!(tx.contains(q).unwrap(), before.contains(q));
    tx.commit().unwrap();
    assert_eq!(committed(&store), before);
}

/// `named_graphs` lists each named graph that holds at least one quad, once.
fn contract_named_graphs<S: Storage>(fresh: impl Fn() -> S, quads: &[Quad]) {
    let store = fresh();
    load(&store, quads);
    let got = store.named_graphs().unwrap();
    let got_set: HashSet<NamedNode> = got.iter().cloned().collect();
    assert_eq!(got.len(), got_set.len(), "named_graphs yielded duplicates");
    let expected: HashSet<NamedNode> = quads
        .iter()
        .filter_map(|q| match &q.graph {
            NamedOrDefault::Named(g) => Some(g.clone()),
            NamedOrDefault::Default => None,
        })
        .collect();
    assert_eq!(got_set, expected);
}

// ---------------------------------------------------------------------------
// Properties against the reference store
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn round_trip(quads in small_quads(24)) {
        contract_round_trip(MemStore::new, &quads);
    }

    #[test]
    fn idempotent_insert(quads in small_quads(24)) {
        contract_idempotent_insert(MemStore::new, &quads);
    }

    #[test]
    fn delete_after_insert(base in small_quads(16), extra in small_quad()) {
        contract_delete_after_insert(MemStore::new, &base, &extra);
    }

    #[test]
    fn pattern_matches_reference(
        quads in small_quads(24),
        s in maybe(small_subject()),
        p in maybe(small_named()),
        o in maybe(small_term()),
        g in maybe(small_graph()),
    ) {
        contract_pattern(MemStore::new, &quads, s.as_ref(), p.as_ref(), o.as_ref(), g.as_ref());
    }

    /// Patterns built from a quad that is in the store always find it.
    #[test]
    fn pattern_from_member_finds_it(
        quads in small_quads(16).prop_filter("non-empty", |v| !v.is_empty()),
        idx in any::<prop::sample::Index>(),
        mask in any::<[bool; 4]>(),
    ) {
        let q = idx.get(&quads).clone();
        let store = MemStore::with_quads(&quads);
        let tx = store.begin_read().unwrap();
        let found = tx
            .quads_for_pattern(
                mask[0].then_some(&q.subject),
                mask[1].then_some(&q.predicate),
                mask[2].then_some(&q.object),
                mask[3].then_some(&q.graph),
            )
            .map(|r| r.unwrap())
            .any(|x| x == q);
        prop_assert!(found);
    }

    #[test]
    fn rollback_and_isolation(base in small_quads(12), writes in small_quads(12)) {
        contract_rollback_and_isolation(MemStore::new, &base, &writes);
    }

    #[test]
    fn read_only_rejects_writes(base in small_quads(12), q in small_quad()) {
        contract_read_only_rejects_writes(MemStore::new, &base, &q);
    }

    #[test]
    fn named_graphs_are_distinct_and_complete(quads in small_quads(24)) {
        contract_named_graphs(MemStore::new, &quads);
    }
}

// ---------------------------------------------------------------------------
// Example-based checks
// ---------------------------------------------------------------------------

#[test]
fn conflicting_commits_report_transaction_conflict() {
    let store = MemStore::new();
    let q = Quad::triple(
        NamedNode::new("https://example.org/s").into(),
        NamedNode::new("https://example.org/p"),
        NamedNode::new("https://example.org/o").into(),
    );
    let mut a = store.begin_write().unwrap();
    let mut b = store.begin_write().unwrap();
    a.insert(&q).unwrap();
    b.insert(&q).unwrap();
    a.commit().unwrap();
    assert!(matches!(b.commit(), Err(StorageError::TransactionConflict)));
    assert_eq!(store.quad_count().unwrap(), 1);
}

#[test]
fn empty_store_is_empty() {
    let store = MemStore::new();
    assert_eq!(store.quad_count().unwrap(), 0);
    assert!(store.named_graphs().unwrap().is_empty());
    assert!(committed(&store).is_empty());
    assert!(store.snapshot().is_empty());
}

/// `Storage` is `Send + Sync` per RFC 0002; a store can be shared across
/// threads by reference.
#[test]
fn storage_is_shareable_across_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MemStore>();

    let store = MemStore::new();
    std::thread::scope(|scope| {
        for i in 0..4 {
            let store = &store;
            scope.spawn(move || {
                let tx = store.begin_read().unwrap();
                let _ = tx.contains(&Quad::triple(
                    NamedNode::new(format!("https://example.org/{i}")).into(),
                    NamedNode::new("https://example.org/p"),
                    NamedNode::new("https://example.org/o").into(),
                ));
                tx.rollback().unwrap();
            });
        }
    });
}
