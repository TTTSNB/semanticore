//! Behaviour specific to the Oxigraph backend: persistence, read-only
//! stores, error mapping, term fidelity, and locking. The backend-neutral
//! contract lives in `tests/contract.rs`.

use std::collections::HashSet;
use std::sync::Barrier;
use std::time::Duration;

use proptest::prelude::*;
use semanticore_core::model::{
    BlankNode, Literal, NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term,
};
use semanticore_core::storage::{Storage, StorageError, StorageTransaction, TransactionIsolation};
use semanticore_storage_testkit::contract::{all_quads, committed, load};
use semanticore_store_oxigraph::{AdapterError, OxigraphStore};

fn ex(s: &str) -> NamedNode {
    NamedNode::new(format!("https://example.org/{s}"))
}

fn triple(s: &str, o: impl Into<Term>) -> Quad {
    Quad::triple(ex(s).into(), ex("p"), o.into())
}

fn sample() -> Vec<Quad> {
    vec![
        triple("a", ex("b")),
        triple("a", Literal::new_lang("colour", "en-GB")),
        triple("b", BlankNode::new("x")),
        Quad::new(
            BlankNode::new("y").into(),
            ex("p"),
            Literal::new_simple("in g").into(),
            NamedOrDefault::Named(ex("g")),
        ),
    ]
}

fn adapter_error(e: &StorageError) -> Option<AdapterError> {
    match e {
        StorageError::Backend(inner) => inner.downcast_ref::<AdapterError>().copied(),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Persistence and opening
// ---------------------------------------------------------------------------

#[test]
fn file_backed_store_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = OxigraphStore::open(dir.path()).unwrap();
        load(&store, &sample());
        // Uncommitted writes must not survive.
        let mut tx = store.begin_write().unwrap();
        tx.insert(&triple("uncommitted", ex("x"))).unwrap();
    }
    let store = OxigraphStore::open(dir.path()).unwrap();
    assert_eq!(committed(&store), sample().into_iter().collect());
    assert_eq!(store.quad_count().unwrap(), 4);
    assert_eq!(store.named_graphs().unwrap(), vec![ex("g")]);
}

#[test]
fn read_only_store_reads_but_refuses_writes() {
    let dir = tempfile::tempdir().unwrap();
    load(&OxigraphStore::open(dir.path()).unwrap(), &sample());

    let store = OxigraphStore::open_read_only(dir.path()).unwrap();
    let err = store.begin_write().unwrap_err();
    assert_eq!(adapter_error(&err), Some(AdapterError::ReadOnlyStore));
    assert_eq!(err.to_string(), "backend: store was opened read-only");

    let mut tx = store.begin_read().unwrap();
    assert_eq!(all_quads(&tx), sample().into_iter().collect());
    let err = tx.insert(&triple("z", ex("z"))).unwrap_err();
    assert_eq!(adapter_error(&err), Some(AdapterError::ReadOnlyTransaction));
    tx.commit().unwrap();

    assert_eq!(store.quad_count().unwrap(), 4);
    assert_eq!(store.named_graphs().unwrap(), vec![ex("g")]);
    let caps = store.capabilities();
    assert!(caps.persistent && !caps.concurrent_writes && !caps.bulk_load);
}

#[test]
fn second_writer_on_same_directory_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let _first = OxigraphStore::open(dir.path()).unwrap();
    let err = OxigraphStore::open(dir.path()).unwrap_err();
    assert!(
        matches!(err, StorageError::Io(_) | StorageError::Backend(_)),
        "{err:?}"
    );
}

#[test]
fn opening_bad_paths_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plain-file");
    std::fs::write(&file, b"not a database").unwrap();
    assert!(OxigraphStore::open(&file).is_err());
    assert!(OxigraphStore::open_read_only(dir.path().join("missing")).is_err());
}

#[test]
fn capabilities_per_variant() {
    let mem = OxigraphStore::in_memory().unwrap().capabilities();
    assert!(!mem.persistent && mem.sparql_endpoint && !mem.bulk_load);
    assert!(!mem.approximate_quad_count);
    assert_eq!(mem.isolation, TransactionIsolation::ReadCommitted);

    let dir = tempfile::tempdir().unwrap();
    let disk = OxigraphStore::open(dir.path()).unwrap().capabilities();
    assert!(disk.persistent && disk.bulk_load && disk.sparql_endpoint);
    assert!(disk.concurrent_writes && !disk.approximate_quad_count);
    assert_eq!(disk.isolation, TransactionIsolation::SnapshotIsolation);
}

#[test]
fn debug_output_names_the_variant_and_mode() {
    let store = OxigraphStore::in_memory().unwrap();
    assert!(format!("{store:?}").contains("InMemory"));
    let r = store.begin_read().unwrap();
    assert!(format!("{r:?}").contains("read (latest)"));
    drop(r);
    let w = store.begin_write().unwrap();
    assert!(format!("{w:?}").contains("write"));
    drop(w);

    let dir = tempfile::tempdir().unwrap();
    let disk = OxigraphStore::open(dir.path()).unwrap();
    let r = disk.begin_read().unwrap();
    assert!(format!("{r:?}").contains("read (snapshot)"));
    r.rollback().unwrap();
}

// ---------------------------------------------------------------------------
// Terms the store cannot hold verbatim
// ---------------------------------------------------------------------------

fn unrepresentable() -> Vec<Quad> {
    let int = |v: &str| {
        Literal::new_typed(
            v,
            NamedNode::new("http://www.w3.org/2001/XMLSchema#integer"),
        )
    };
    vec![
        Quad::triple(NamedNode::new("not an iri").into(), ex("p"), ex("o").into()),
        Quad::triple(ex("s").into(), NamedNode::new("rel/ative"), ex("o").into()),
        triple("s", BlankNode::new("bad label")),
        triple("s", Literal::new_lang("x", "")),
        triple("s", int("01")),
        triple(
            "s",
            Literal::new_typed(
                "x",
                NamedNode::new("http://www.w3.org/1999/02/22-rdf-syntax-ns#langString"),
            ),
        ),
        Quad::new(
            ex("s").into(),
            ex("p"),
            ex("o").into(),
            NamedOrDefault::Named(NamedNode::new("no scheme")),
        ),
    ]
}

#[test]
fn unrepresentable_quads_are_refused_and_never_found() {
    let store = OxigraphStore::in_memory().unwrap();
    load(&store, &sample());
    let before = committed(&store);

    let mut tx = store.begin_write().unwrap();
    for q in unrepresentable() {
        let err = tx.insert(&q).unwrap_err();
        assert!(
            matches!(err, StorageError::Serialization(_)),
            "{q:?}: {err:?}"
        );
        assert!(!tx.contains(&q).unwrap());
        assert!(!tx.remove(&q).unwrap());
        let hits = tx
            .quads_for_pattern(
                Some(&q.subject),
                Some(&q.predicate),
                Some(&q.object),
                Some(&q.graph),
            )
            .count();
        assert_eq!(hits, 0);
    }
    tx.commit().unwrap();
    assert_eq!(committed(&store), before);
}

#[test]
fn bound_unrepresentable_term_matches_nothing_in_each_position() {
    let store = OxigraphStore::in_memory().unwrap();
    load(&store, &sample());
    let tx = store.begin_read().unwrap();
    let bad_iri = NamedNode::new("bad iri");
    let bad_s: NamedOrBlankNode = bad_iri.clone().into();
    let bad_o: Term = bad_iri.clone().into();
    let bad_g = NamedOrDefault::Named(bad_iri.clone());
    assert_eq!(
        tx.quads_for_pattern(Some(&bad_s), None, None, None).count(),
        0
    );
    assert_eq!(
        tx.quads_for_pattern(None, Some(&bad_iri), None, None)
            .count(),
        0
    );
    assert_eq!(
        tx.quads_for_pattern(None, None, Some(&bad_o), None).count(),
        0
    );
    assert_eq!(
        tx.quads_for_pattern(None, None, None, Some(&bad_g)).count(),
        0
    );
    assert_eq!(tx.quads_for_pattern(None, None, None, None).count(), 4);
}

#[test]
fn non_canonical_literal_does_not_alias_its_canonical_form() {
    let int = |v: &str| {
        Literal::new_typed(
            v,
            NamedNode::new("http://www.w3.org/2001/XMLSchema#integer"),
        )
    };
    let store = OxigraphStore::in_memory().unwrap();
    load(&store, &[triple("s", int("1"))]);
    let tx = store.begin_read().unwrap();
    assert!(tx.contains(&triple("s", int("1"))).unwrap());
    // Oxigraph alone would report "01" as present: both encode to 1.
    assert!(!tx.contains(&triple("s", int("01"))).unwrap());
}

#[test]
fn foreign_data_the_core_model_cannot_carry_is_an_error_on_read() {
    use oxigraph::model as ox;
    let store = OxigraphStore::in_memory().unwrap();
    let n = ox::NamedNode::new("https://example.org/n").unwrap();
    store
        .oxigraph()
        .insert(&ox::Quad::new(
            n.clone(),
            n.clone(),
            n.clone(),
            ox::BlankNode::new("g").unwrap(),
        ))
        .unwrap();

    let tx = store.begin_read().unwrap();
    let items: Vec<_> = tx.quads_for_pattern(None, None, None, None).collect();
    assert_eq!(items.len(), 1);
    assert!(matches!(&items[0], Err(StorageError::Serialization(m)) if m.contains("_:g")));
    assert!(matches!(
        store.named_graphs(),
        Err(StorageError::Serialization(_))
    ));
}

// Every literal either refuses to go in, or comes back exactly as it went
// in. This checks the adapter's model of Oxigraph's literal encoding against
// the real store.
fn xsd(local: &str) -> NamedNode {
    NamedNode::new(format!("http://www.w3.org/2001/XMLSchema#{local}"))
}

fn tricky_literal() -> impl Strategy<Value = Literal> {
    let datatypes = prop::sample::select(vec![
        "string",
        "boolean",
        "integer",
        "int",
        "long",
        "unsignedByte",
        "nonNegativeInteger",
        "decimal",
        "float",
        "double",
        "dateTime",
        "dateTimeStamp",
        "date",
        "time",
        "gYear",
        "gYearMonth",
        "gMonth",
        "gMonthDay",
        "gDay",
        "duration",
        "dayTimeDuration",
        "yearMonthDuration",
        "anyURI",
        "token",
    ]);
    let values = prop::sample::select(vec![
        "",
        "0",
        "1",
        "01",
        "+1",
        "-0",
        "-1",
        " 1",
        "1.0",
        "1.50",
        ".5",
        "1e3",
        "1E3",
        "1.0E0",
        "INF",
        "-INF",
        "NaN",
        "true",
        "false",
        "TRUE",
        "abc",
        "2024-01-01",
        "2024-1-1",
        "2024-01-01T00:00:00",
        "2024-01-01T00:00:00Z",
        "2024-01-01T00:00:00.000Z",
        "2024-01-01T00:00:00+00:00",
        "12:00:00",
        "12:00:00.50",
        "2024",
        "02024",
        "2024-05",
        "--05",
        "--05-01",
        "---01",
        "P1D",
        "PT24H",
        "P1Y12M",
        "P0D",
        "PT0S",
        "-P1D",
    ]);
    prop_oneof![
        (values.clone(), datatypes).prop_map(|(v, d)| Literal::new_typed(v, xsd(d))),
        (
            values,
            prop::sample::select(vec!["en", "en-GB", "EN", "zh-Hant-TW", "x-private", "1"])
        )
            .prop_map(|(v, l)| Literal::new_lang(v, l)),
    ]
}

proptest! {
    #[test]
    fn literal_round_trip(lits in prop::collection::vec(tricky_literal(), 1..16)) {
        let store = OxigraphStore::in_memory().unwrap();
        let mut accepted = HashSet::new();
        let mut tx = store.begin_write().unwrap();
        for l in lits {
            let q = triple("s", l);
            match tx.insert(&q) {
                Ok(_) => { accepted.insert(q); }
                Err(StorageError::Serialization(_)) => prop_assert!(!tx.contains(&q).unwrap()),
                Err(e) => prop_assert!(false, "unexpected error {e:?}"),
            }
        }
        tx.commit().unwrap();
        prop_assert_eq!(committed(&store), accepted.clone());
        let tx = store.begin_read().unwrap();
        for q in &accepted {
            prop_assert!(tx.contains(q).unwrap());
            let hits: Vec<Quad> = tx
                .quads_for_pattern(None, None, Some(&q.object), None)
                .map(|r| r.unwrap())
                .collect();
            prop_assert_eq!(hits, vec![q.clone()]);
        }
    }
}

// ---------------------------------------------------------------------------
// Locking and concurrency
// ---------------------------------------------------------------------------

/// In memory, a second writer waits for the first to finish, then sees its
/// commit (Oxigraph's global writer lock).
#[test]
fn in_memory_writers_are_serialized() {
    let store = OxigraphStore::in_memory().unwrap();
    let q = triple("s", ex("o"));
    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let mut tx = store.begin_write().unwrap();
            barrier.wait();
            assert!(tx.insert(&q).unwrap());
            std::thread::sleep(Duration::from_millis(100));
            tx.commit().unwrap();
        });
        scope.spawn(|| {
            barrier.wait();
            let mut tx = store.begin_write().unwrap();
            assert!(
                !tx.insert(&q).unwrap(),
                "second writer did not see the first commit"
            );
            tx.commit().unwrap();
        });
    });
    assert_eq!(store.quad_count().unwrap(), 1);
}

/// On disk, concurrent writers do not block and are not checked for
/// conflicts: both commit, and the last commit wins per quad.
#[test]
fn file_backed_writers_do_not_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let store = OxigraphStore::open(dir.path()).unwrap();
    let (q1, q2) = (triple("one", ex("o")), triple("two", ex("o")));
    load(&store, std::slice::from_ref(&q1));

    let mut a = store.begin_write().unwrap();
    let mut b = store.begin_write().unwrap();
    assert!(a.remove(&q1).unwrap());
    assert!(b.insert(&q2).unwrap());
    assert!(
        !b.insert(&q1).unwrap(),
        "b reads its own snapshot, where q1 exists"
    );
    a.commit().unwrap();
    b.commit().unwrap();

    assert_eq!(committed(&store), [q2].into_iter().collect());
}

/// The in-memory read path is read committed: a reader started before a
/// commit sees it on its next call.
#[test]
fn in_memory_reads_see_latest_commit() {
    let store = OxigraphStore::in_memory().unwrap();
    let reader = store.begin_read().unwrap();
    assert_eq!(all_quads(&reader).len(), 0);
    load(&store, &sample());
    assert_eq!(all_quads(&reader).len(), 4);
    reader.rollback().unwrap();
}

#[test]
fn adapter_error_display() {
    assert_eq!(
        AdapterError::ReadOnlyTransaction.to_string(),
        "transaction is read-only"
    );
    assert_eq!(
        AdapterError::ReadOnlyStore.to_string(),
        "store was opened read-only"
    );
}
