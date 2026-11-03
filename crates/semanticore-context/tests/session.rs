//! `Session` lifecycle, `SessionConfig` defaults and storage wiring.

mod common;

use std::time::Duration;

use common::*;
use semanticore_context::{
    ContextScope, DemotionTarget, EvictionPolicy, FlushStrategy, PersistenceTarget,
    PromotionCadence, ReasonerSelection, Session, SessionConfig, SessionError, StorageConfig,
};
use semanticore_core::storage::{Storage, StorageCapability};
use semanticore_storage_testkit::MemStore;
use semanticore_store_oxigraph::OxigraphStore;

#[test]
fn session_config_defaults_follow_the_rfc() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    assert!(matches!(cfg.reasoners, ReasonerSelection::Auto));
    assert_eq!(cfg.bounds.enduring_max_items, None);
    assert_eq!(cfg.bounds.major_max_items, None);
    assert_eq!(cfg.bounds.minor_max_items, None);
    assert_eq!(cfg.eviction_policy, EvictionPolicy::Lru);
    assert_eq!(cfg.demotion_target, DemotionTarget::OneStepThenDrop);
    assert_eq!(cfg.persistence.target, PersistenceTarget::EnduringStorage);
    assert_eq!(cfg.persistence.flush_strategy, FlushStrategy::OnCommit);
    // RFC 0004 "Promotion / demotion semantics" defaults.
    assert_eq!(cfg.promotion.cadence, PromotionCadence::AfterEachQuery);
    assert_eq!(cfg.promotion.minor_to_major_accesses, 3);
    assert_eq!(cfg.promotion.minor_window, Duration::from_secs(60));
    assert_eq!(cfg.promotion.major_to_enduring_references, 5);
}

#[test]
fn oxigraph_config_is_file_backed_enduring_and_in_memory_elsewhere() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let StorageConfig::PerScope {
        enduring,
        major,
        minor,
    } = &cfg.storage
    else {
        panic!("expected per-scope storage");
    };
    assert!(enduring.capabilities().persistent);
    assert!(!major.capabilities().persistent);
    assert!(!minor.capabilities().persistent);
}

#[test]
fn open_refuses_a_non_persistent_enduring_store() {
    let cfg = SessionConfig::new(StorageConfig::PerScope {
        enduring: MemStore::new(),
        major: MemStore::new(),
        minor: MemStore::new(),
    });
    match Session::open(cfg) {
        Err(SessionError::CapabilityShortfall { scope, required }) => {
            assert_eq!(scope, ContextScope::Enduring);
            assert_eq!(required, StorageCapability::Persistent);
        }
        Err(e) => panic!("unexpected error {e}"),
        Ok(_) => panic!("open must fail"),
    }
}

#[test]
fn open_accepts_any_storage_backend_with_a_persistent_enduring_store() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = SessionConfig::new(StorageConfig::PerScope {
        enduring: OxigraphStore::open(dir.path()).unwrap(),
        major: OxigraphStore::in_memory().unwrap(),
        minor: OxigraphStore::in_memory().unwrap(),
    });
    let mut s = Session::open(cfg).unwrap();
    let id = s.add(ContextScope::Major, fact("a", "p", "b")).unwrap();
    assert_eq!(s.scope_of(id), Some(ContextScope::Major));
}

#[test]
fn close_reports_session_stats() {
    let (_dir, mut s) = session();
    s.add(ContextScope::Minor, fact("a", "p", "b")).unwrap();
    s.add_batch(
        ContextScope::Major,
        &[fact("c", "p", "d"), fact("e", "p", "f")],
    )
    .unwrap();
    s.query(everything_in(ContextScope::Major)).unwrap();
    s.query(everything_in(ContextScope::Minor)).unwrap();
    let stats = s.close().unwrap();
    assert_eq!(stats.items_added, 3);
    assert_eq!(stats.queries_served, 2);
    assert_eq!(stats.reasoning_passes, 0);
    assert_eq!(stats.items_promoted, 0);
    assert_eq!(stats.items_demoted, 0);
    assert_eq!(stats.items_evicted, 0);
}

#[test]
fn enduring_items_survive_close_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let event = semanticore_context::ContextItem::event(
        "tool_call",
        serde_json::json!({"tool": "search", "n": 3}),
        prov(),
    )
    .with_embedding(vec![0.25, -1.5]);
    let chunk = semanticore_context::ContextItem::chunk("hello", "text/plain", prov())
        .with_scope_hint(ContextScope::Major);
    let (fact_id, event_id, chunk_id) = {
        let mut s = Session::open(config(dir.path())).unwrap();
        let f = s.add(ContextScope::Enduring, fact("a", "p", "b")).unwrap();
        let e = s.add(ContextScope::Enduring, event.clone()).unwrap();
        let c = s.add(ContextScope::Enduring, chunk.clone()).unwrap();
        s.add(ContextScope::Major, fact("major", "p", "x")).unwrap();
        s.add(ContextScope::Minor, fact("minor", "p", "x")).unwrap();
        s.close().unwrap();
        (f, e, c)
    };

    let s = Session::open(config(dir.path())).unwrap();
    assert_eq!(s.item_count(ContextScope::Enduring), 3);
    assert_eq!(s.item_count(ContextScope::Major), 0, "major is in-memory");
    assert_eq!(s.item_count(ContextScope::Minor), 0, "minor is in-memory");

    let snap = s.snapshot(ContextScope::Enduring).unwrap();
    let find = |id| {
        snap.items
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, item)| item.clone())
            .expect("item reloaded")
    };
    assert_eq!(find(fact_id), fact("a", "p", "b"));
    assert_eq!(find(event_id), event);
    assert_eq!(find(chunk_id), chunk);
    // The reloaded triples are queryable again.
    assert_eq!(
        ids(&s, subject_in(ContextScope::Enduring, "a")),
        vec![fact_id]
    );
}

#[test]
fn ids_issued_after_reopen_do_not_collide_with_reloaded_items() {
    let dir = tempfile::tempdir().unwrap();
    let old = {
        let mut s = Session::open(config(dir.path())).unwrap();
        let id = s.add(ContextScope::Enduring, fact("a", "p", "b")).unwrap();
        s.close().unwrap();
        id
    };
    let mut s = Session::open(config(dir.path())).unwrap();
    let new = s.add(ContextScope::Enduring, fact("c", "p", "d")).unwrap();
    assert_ne!(old, new);
    assert_eq!(s.item_count(ContextScope::Enduring), 2);
}

#[test]
fn removed_enduring_items_stay_removed_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut s = Session::open(config(dir.path())).unwrap();
        let id = s.add(ContextScope::Enduring, fact("a", "p", "b")).unwrap();
        assert!(s.remove(ContextScope::Enduring, &id).unwrap());
        s.close().unwrap();
    }
    let s = Session::open(config(dir.path())).unwrap();
    assert_eq!(s.item_count(ContextScope::Enduring), 0);
    // Neither the payload nor the metadata record is left on disk.
    drop(s);
    let store = OxigraphStore::open(dir.path()).unwrap();
    assert_eq!(store.quad_count().unwrap(), 0);
}

#[test]
fn snapshot_lists_items_in_id_order_with_the_tier_quad_count() {
    let (_dir, mut s) = session();
    let a = s.add(ContextScope::Major, fact("a", "p", "b")).unwrap();
    let b = s
        .add(
            ContextScope::Major,
            semanticore_context::ContextItem::triples(
                vec![triple("c", "p", ex("d")), triple("c", "q", ex("e"))],
                prov(),
            ),
        )
        .unwrap();
    s.add(ContextScope::Minor, fact("x", "p", "y")).unwrap();
    let snap = s.snapshot(ContextScope::Major).unwrap();
    assert_eq!(snap.scope, ContextScope::Major);
    let got: Vec<_> = snap.items.iter().map(|(id, _)| *id).collect();
    assert_eq!(got, vec![a, b]);
    // Three payload quads plus one metadata record per item.
    assert_eq!(snap.quad_count, 5);
}

#[test]
fn session_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Session<OxigraphStore>>();
}
