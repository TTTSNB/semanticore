//! The RFC 0002 `Storage` / `StorageTransaction` contract as plain functions.
//!
//! Every check is generic over `S: Storage` and takes a fresh-store factory,
//! so any backend can run the identical suite. Checks panic (via `assert!`
//! or `unwrap`) on violation; call them from `#[test]` functions or proptest
//! bodies, or use [`crate::storage_contract_suite!`] to generate the lot.

use std::collections::HashSet;

use semanticore_core::model::{NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term};
use semanticore_core::storage::{Storage, StorageTransaction, TransactionIsolation};

use crate::matches_pattern;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Every quad currently visible to `tx`, via the all-wildcard pattern.
pub fn all_quads<T: StorageTransaction>(tx: &T) -> HashSet<Quad> {
    tx.quads_for_pattern(None, None, None, None)
        .map(|r| r.expect("pattern iteration failed"))
        .collect()
}

/// Committed state, read through a fresh read transaction.
pub fn committed<S: Storage>(store: &S) -> HashSet<Quad> {
    let tx = store.begin_read().unwrap();
    let out = all_quads(&tx);
    tx.rollback().unwrap();
    out
}

/// Insert `quads` in one committed write transaction.
pub fn load<S: Storage>(store: &S, quads: &[Quad]) {
    let mut tx = store.begin_write().unwrap();
    for q in quads {
        tx.insert(q).unwrap();
    }
    tx.commit().unwrap();
}

/// Remove `quads` in one committed write transaction.
pub fn unload<S: Storage>(store: &S, quads: &[Quad]) {
    let mut tx = store.begin_write().unwrap();
    for q in quads {
        tx.remove(q).unwrap();
    }
    tx.commit().unwrap();
}

fn graphs_of<'a>(quads: impl IntoIterator<Item = &'a Quad>) -> HashSet<NamedNode> {
    quads
        .into_iter()
        .filter_map(|q| match &q.graph {
            NamedOrDefault::Named(g) => Some(g.clone()),
            NamedOrDefault::Default => None,
        })
        .collect()
}

fn iri(s: &str) -> NamedNode {
    NamedNode::new(format!("https://example.org/{s}"))
}

// ---------------------------------------------------------------------------
// Property-style checks (driven by generated inputs)
// ---------------------------------------------------------------------------

/// Round trip: inserting quads and committing makes exactly that set
/// visible, through `contains`, the wildcard pattern, and `quad_count`.
pub fn round_trip<S: Storage>(fresh: impl Fn() -> S, quads: &[Quad]) {
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
pub fn idempotent_insert<S: Storage>(fresh: impl Fn() -> S, quads: &[Quad]) {
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
pub fn delete_after_insert<S: Storage>(fresh: impl Fn() -> S, base: &[Quad], extra: &Quad) {
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
/// semantics (`None` = wildcard), without duplicates.
pub fn pattern<S: Storage>(
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

/// A pattern built from a stored quad (each position either bound to that
/// quad's term or left as a wildcard, per `mask`) always finds it.
pub fn pattern_from_member<S: Storage>(
    fresh: impl Fn() -> S,
    quads: &[Quad],
    q: &Quad,
    mask: [bool; 4],
) {
    let store = fresh();
    load(&store, quads);
    let tx = store.begin_read().unwrap();
    let found = tx
        .quads_for_pattern(
            mask[0].then_some(&q.subject),
            mask[1].then_some(&q.predicate),
            mask[2].then_some(&q.object),
            mask[3].then_some(&q.graph),
        )
        .map(|r| r.unwrap())
        .any(|x| &x == q);
    assert!(
        found,
        "pattern from member did not find {q:?} (mask {mask:?})"
    );
}

/// Rollback discards every write; uncommitted writes are visible inside the
/// writing transaction (read-your-writes, also through `quads_for_pattern`)
/// but not to a reader that starts while the writer is open.
pub fn rollback_and_isolation<S: Storage>(fresh: impl Fn() -> S, base: &[Quad], writes: &[Quad]) {
    let store = fresh();
    load(&store, base);
    let before = committed(&store);

    let mut tx = store.begin_write().unwrap();
    let mut expected_inside = before.clone();
    for q in writes {
        tx.insert(q).unwrap();
        expected_inside.insert(q.clone());
        assert!(tx.contains(q).unwrap(), "read-your-writes violated");
    }
    if let Some(first) = base.first() {
        tx.remove(first).unwrap();
        expected_inside.remove(first);
        assert!(
            !tx.contains(first).unwrap(),
            "read-your-writes violated (remove)"
        );
    }
    assert_eq!(
        all_quads(&tx),
        expected_inside,
        "read-your-writes violated (pattern)"
    );
    assert_eq!(committed(&store), before, "uncommitted writes leaked");
    tx.rollback().unwrap();

    assert_eq!(committed(&store), before);
}

/// Dropping a write transaction without `commit` behaves like `rollback`.
pub fn drop_discards_writes<S: Storage>(fresh: impl Fn() -> S, base: &[Quad], writes: &[Quad]) {
    let store = fresh();
    load(&store, base);
    let before = committed(&store);
    {
        let mut tx = store.begin_write().unwrap();
        for q in writes {
            tx.insert(q).unwrap();
        }
        for q in base {
            tx.remove(q).unwrap();
        }
    }
    assert_eq!(committed(&store), before);
    assert_eq!(store.quad_count().unwrap(), before.len() as u64);
}

/// Read-only transactions reject writes with an error (not a panic) and
/// leave the store untouched. Reads keep working after the rejection, and a
/// read-only transaction may be committed or rolled back.
pub fn read_only_rejects_writes<S: Storage>(fresh: impl Fn() -> S, base: &[Quad], q: &Quad) {
    let store = fresh();
    load(&store, base);
    let before = committed(&store);

    let mut tx = store.begin_read().unwrap();
    assert!(tx.insert(q).is_err(), "read-only insert must fail");
    assert!(tx.remove(q).is_err(), "read-only remove must fail");
    assert_eq!(tx.contains(q).unwrap(), before.contains(q));
    assert_eq!(all_quads(&tx), before);
    tx.commit().unwrap();
    assert_eq!(committed(&store), before);

    let mut tx = store.begin_read().unwrap();
    if let Some(first) = base.first() {
        assert!(tx.remove(first).is_err());
    }
    tx.rollback().unwrap();
    assert_eq!(committed(&store), before);
}

/// `named_graphs` lists each named graph that holds at least one quad, once.
pub fn named_graphs<S: Storage>(fresh: impl Fn() -> S, quads: &[Quad]) {
    let store = fresh();
    load(&store, quads);
    let got = store.named_graphs().unwrap();
    let got_set: HashSet<NamedNode> = got.iter().cloned().collect();
    assert_eq!(got.len(), got_set.len(), "named_graphs yielded duplicates");
    assert_eq!(got_set, graphs_of(quads));
}

/// After removing quads, `named_graphs` still lists exactly the graphs that
/// hold at least one quad: a graph emptied by removal disappears.
pub fn named_graphs_track_removals<S: Storage>(
    fresh: impl Fn() -> S,
    quads: &[Quad],
    remove: &[bool],
) {
    let store = fresh();
    load(&store, quads);
    let removed: Vec<Quad> = quads
        .iter()
        .zip(remove.iter().cycle())
        .filter(|(_, r)| **r)
        .map(|(q, _)| q.clone())
        .collect();
    unload(&store, &removed);

    let remaining = committed(&store);
    let mut got = store.named_graphs().unwrap();
    let n = got.len();
    got.sort();
    got.dedup();
    assert_eq!(got.len(), n, "named_graphs yielded duplicates");
    let got: HashSet<NamedNode> = got.into_iter().collect();
    assert_eq!(got, graphs_of(&remaining));
    assert_eq!(store.quad_count().unwrap(), remaining.len() as u64);
}

/// A read transaction that started before a commit keeps seeing the state
/// it started with. Only checked when the backend advertises snapshot
/// isolation or stronger; for `ReadCommitted` backends it checks the
/// weaker guarantee that a reader started after the commit sees it.
pub fn snapshot_reads<S: Storage>(fresh: impl Fn() -> S, base: &[Quad], writes: &[Quad]) {
    let store = fresh();
    load(&store, base);
    let before = committed(&store);
    let snapshot = !matches!(
        store.capabilities().isolation,
        TransactionIsolation::ReadCommitted
    );

    let reader = store.begin_read().unwrap();
    // Touch the reader first so lazily-started snapshots are pinned too.
    assert_eq!(all_quads(&reader), before);

    let mut tx = store.begin_write().unwrap();
    for q in writes {
        tx.insert(q).unwrap();
    }
    if let Some(first) = base.first() {
        tx.remove(first).unwrap();
    }
    tx.commit().unwrap();
    let after = committed(&store);

    if snapshot {
        assert_eq!(all_quads(&reader), before, "snapshot moved under a reader");
        for q in writes {
            assert_eq!(reader.contains(q).unwrap(), before.contains(q));
        }
    }
    reader.rollback().unwrap();

    let mut expected = before;
    expected.extend(writes.iter().cloned());
    if let Some(first) = base.first() {
        expected.remove(first);
    }
    assert_eq!(after, expected);
}

// ---------------------------------------------------------------------------
// Example-based checks
// ---------------------------------------------------------------------------

/// A fresh store is empty.
pub fn empty_store_is_empty<S: Storage>(fresh: impl Fn() -> S) {
    let store = fresh();
    assert_eq!(store.quad_count().unwrap(), 0);
    assert!(store.named_graphs().unwrap().is_empty());
    assert!(committed(&store).is_empty());
}

/// The same triple in the default graph and in two named graphs is three
/// distinct quads; the graph slot filters patterns and removal is per graph.
pub fn graphs_are_separate<S: Storage>(fresh: impl Fn() -> S) {
    let store = fresh();
    let (s, p, o) = (iri("s"), iri("p"), iri("o"));
    let (g1, g2) = (iri("g1"), iri("g2"));
    let in_graph = |g: NamedOrDefault| Quad::new(s.clone().into(), p.clone(), o.clone().into(), g);
    let qd = in_graph(NamedOrDefault::Default);
    let q1 = in_graph(NamedOrDefault::Named(g1.clone()));
    let q2 = in_graph(NamedOrDefault::Named(g2.clone()));
    load(&store, &[qd.clone(), q1.clone(), q2.clone()]);
    assert_eq!(store.quad_count().unwrap(), 3);

    let tx = store.begin_read().unwrap();
    let only = |g: &NamedOrDefault| -> Vec<Quad> {
        tx.quads_for_pattern(None, None, None, Some(g))
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(only(&NamedOrDefault::Default), vec![qd.clone()]);
    assert_eq!(only(&NamedOrDefault::Named(g1.clone())), vec![q1.clone()]);
    assert_eq!(only(&NamedOrDefault::Named(iri("nope"))), vec![]);
    let same_triple: HashSet<Quad> = tx
        .quads_for_pattern(
            Some(&s.clone().into()),
            Some(&p),
            Some(&o.clone().into()),
            None,
        )
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(same_triple.len(), 3);
    tx.rollback().unwrap();

    let mut graphs = store.named_graphs().unwrap();
    graphs.sort();
    assert_eq!(graphs, vec![g1.clone(), g2.clone()]);

    unload(&store, std::slice::from_ref(&q1));
    assert_eq!(
        committed(&store),
        [qd, q2].into_iter().collect::<HashSet<_>>()
    );
    assert_eq!(store.named_graphs().unwrap(), vec![g2]);
}

/// After a rollback or a dropped transaction, the store accepts and commits
/// new writes (no lock or other state leaks out of a finished transaction).
pub fn writes_after_rollback<S: Storage>(fresh: impl Fn() -> S) {
    let store = fresh();
    let q = |n: &str| Quad::triple(iri(n).into(), iri("p"), iri("o").into());

    let mut tx = store.begin_write().unwrap();
    tx.insert(&q("a")).unwrap();
    tx.rollback().unwrap();

    {
        let mut tx = store.begin_write().unwrap();
        tx.insert(&q("b")).unwrap();
    }

    let mut tx = store.begin_write().unwrap();
    assert!(tx.insert(&q("c")).unwrap());
    tx.commit().unwrap();

    assert_eq!(committed(&store), [q("c")].into_iter().collect());
}

/// `Storage` is `Send + Sync` (RFC 0002): readers on several threads can
/// share one store while another thread commits.
pub fn shareable_across_threads<S: Storage>(fresh: impl Fn() -> S) {
    let store = fresh();
    let q = |i: usize| Quad::triple(iri(&format!("s{i}")).into(), iri("p"), iri("o").into());
    std::thread::scope(|scope| {
        let store = &store;
        scope.spawn(move || {
            for i in 0..8 {
                load(store, &[q(i)]);
            }
        });
        for _ in 0..4 {
            scope.spawn(move || {
                for _ in 0..8 {
                    let tx = store.begin_read().unwrap();
                    let n = all_quads(&tx).len();
                    assert!(n <= 8);
                    tx.rollback().unwrap();
                }
            });
        }
    });
    assert_eq!(store.quad_count().unwrap(), 8);
}
