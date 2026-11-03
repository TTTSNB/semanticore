//! Promotion rules (RFC 0004 "Promotion / demotion semantics"): explicit
//! `Session::promote` and the two auto-promotion rules.

mod common;

use std::time::Duration;

use common::*;
use semanticore_context::{ContextScope, ItemId, PromotionCadence, Session, SessionError};
use semanticore_store_oxigraph::OxigraphStore;

use ContextScope::{Enduring, Major, Minor};

// ---------------------------------------------------------------------------
// ContextScope durability order
// ---------------------------------------------------------------------------

#[test]
fn scopes_are_ordered_by_durability() {
    assert!(Minor < Major && Major < Enduring);
    assert_eq!(ContextScope::ALL, [Enduring, Major, Minor]);
    assert_eq!(Enduring.colder(), Some(Major));
    assert_eq!(Major.colder(), Some(Minor));
    assert_eq!(Minor.colder(), None);
}

// ---------------------------------------------------------------------------
// Explicit promotion
// ---------------------------------------------------------------------------

#[test]
fn promote_moves_items_to_a_more_durable_scope() {
    let (_dir, mut s) = session();
    let a = s.add(Minor, fact("a", "p", "b")).unwrap();
    let b = s.add(Minor, fact("a", "p", "c")).unwrap();
    let stats = s.promote(&[a, b], Minor, Major).unwrap();
    assert_eq!(stats.promoted, 2);
    assert_eq!(s.scope_of(a), Some(Major));
    assert_eq!(s.scope_of(b), Some(Major));
    // The payload moved with the item: visible in major, gone from minor.
    assert_eq!(ids(&s, subject_in(Major, "a")), vec![a, b]);
    assert!(ids(&s, subject_in(Minor, "a")).is_empty());
    assert_eq!(s.close().unwrap().items_promoted, 2);
}

#[test]
fn promote_may_skip_a_tier() {
    let (_dir, mut s) = session();
    let a = s.add(Minor, fact("a", "p", "b")).unwrap();
    s.promote(&[a], Minor, Enduring).unwrap();
    assert_eq!(s.scope_of(a), Some(Enduring));
}

#[test]
fn promote_within_the_same_scope_is_a_no_op() {
    let (_dir, mut s) = session();
    let a = s.add(Major, fact("a", "p", "b")).unwrap();
    assert_eq!(s.promote(&[a], Major, Major).unwrap().promoted, 0);
    assert_eq!(s.scope_of(a), Some(Major));
}

#[test]
fn promote_refuses_a_less_durable_target() {
    let (_dir, mut s) = session();
    let a = s.add(Enduring, fact("a", "p", "b")).unwrap();
    match s.promote(&[a], Enduring, Minor) {
        Err(SessionError::DurabilityViolation { from, to }) => {
            assert_eq!((from, to), (Enduring, Minor));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(s.scope_of(a), Some(Enduring));
}

#[test]
fn promote_refuses_items_not_in_the_source_scope_and_moves_nothing() {
    let (_dir, mut s) = session();
    let a = s.add(Minor, fact("a", "p", "b")).unwrap();
    let m = s.add(Major, fact("m", "p", "b")).unwrap();
    match s.promote(&[a, m], Minor, Enduring) {
        Err(SessionError::ScopeMismatch {
            item,
            expected,
            found,
        }) => {
            assert_eq!((item, expected, found), (m, Minor, Major));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(s.scope_of(a), Some(Minor), "nothing moved");
}

#[test]
fn promote_refuses_unknown_items() {
    let (_dir, mut s) = session();
    let a = s.add(Minor, fact("a", "p", "b")).unwrap();
    s.remove(Minor, &a).unwrap();
    assert!(matches!(
        s.promote(&[a], Minor, Major),
        Err(SessionError::UnknownItem(id)) if id == a
    ));
}

#[test]
fn items_promoted_to_enduring_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut s = Session::open(config(dir.path())).unwrap();
        let a = s.add(Major, fact("a", "p", "b")).unwrap();
        s.promote(&[a], Major, Enduring).unwrap();
        s.close().unwrap();
    }
    let s = Session::open(config(dir.path())).unwrap();
    assert_eq!(ids(&s, subject_in(Enduring, "a")).len(), 1);
}

// ---------------------------------------------------------------------------
// Auto-promotion: minor -> major on access frequency
// ---------------------------------------------------------------------------

#[test]
fn a_minor_item_accessed_more_than_the_threshold_auto_promotes_to_major() {
    let (_dir, mut s) = session();
    let a = s.add(Minor, fact("a", "p", "b")).unwrap();
    // Default threshold is 3: three accesses are not "more than 3".
    for _ in 0..3 {
        assert_eq!(ids(&s, subject_in(Minor, "a")), vec![a]);
        assert_eq!(s.scope_of(a), Some(Minor));
    }
    let res = s.query(subject_in(Minor, "a")).unwrap();
    assert_eq!(res.items[0].source_scope, Minor, "served from minor");
    assert_eq!(res.query_stats.auto_promoted, 1);
    assert_eq!(s.scope_of(a), Some(Major));
    assert_eq!(ids(&s, subject_in(Major, "a")), vec![a]);
    assert_eq!(s.close().unwrap().items_promoted, 1);
}

#[test]
fn only_returned_items_count_as_accessed() {
    let (_dir, mut s) = session();
    let a = s.add(Minor, fact("a", "p", "b")).unwrap();
    let b = s.add(Minor, fact("b", "p", "c")).unwrap();
    for _ in 0..4 {
        s.query(subject_in(Minor, "a")).unwrap();
    }
    assert_eq!(s.scope_of(a), Some(Major));
    assert_eq!(s.scope_of(b), Some(Minor));
}

#[test]
fn items_cut_by_the_limit_are_not_accessed() {
    let (_dir, mut s) = session();
    let a = s.add(Minor, fact("x", "p", "a")).unwrap();
    let b = s.add(Minor, fact("x", "p", "b")).unwrap();
    for _ in 0..4 {
        s.query(subject_in(Minor, "x").with_limit(1)).unwrap();
    }
    assert_eq!(s.scope_of(a), Some(Major));
    assert_eq!(s.scope_of(b), Some(Minor));
}

#[test]
fn accesses_outside_the_window_do_not_count() {
    let (_dir, mut s) = session_with(|c| c.promotion.minor_window = Duration::from_millis(1));
    let a = s.add(Minor, fact("a", "p", "b")).unwrap();
    for _ in 0..6 {
        s.query(subject_in(Minor, "a")).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(s.scope_of(a), Some(Minor));
}

#[test]
fn the_access_threshold_is_configurable() {
    let (_dir, mut s) = session_with(|c| c.promotion.minor_to_major_accesses = 0);
    let a = s.add(Minor, fact("a", "p", "b")).unwrap();
    s.query(subject_in(Minor, "a")).unwrap();
    assert_eq!(s.scope_of(a), Some(Major));
}

#[test]
fn accesses_in_major_do_not_auto_promote_to_enduring() {
    let (_dir, mut s) = session();
    let a = s.add(Major, fact("a", "p", "b")).unwrap();
    for _ in 0..10 {
        s.query(subject_in(Major, "a")).unwrap();
    }
    assert_eq!(s.scope_of(a), Some(Major));
}

// ---------------------------------------------------------------------------
// Auto-promotion: major -> enduring on reference count
// ---------------------------------------------------------------------------

fn add_citing(s: &mut Session<OxigraphStore>, scope: ContextScope, cited: ItemId, n: usize) {
    for i in 0..n {
        let mut item = fact(&format!("cite{i}"), "p", "x");
        item.provenance = prov().with_derived_from([cited]);
        s.add(scope, item).unwrap();
    }
}

#[test]
fn a_major_item_referenced_by_more_than_the_threshold_auto_promotes_to_enduring() {
    let (_dir, mut s) = session();
    let hub = s.add(Major, fact("hub", "p", "x")).unwrap();
    add_citing(&mut s, Minor, hub, 5);
    // Promotion runs at the end of a query, whatever the query returned.
    s.query(subject_in(Enduring, "nothing")).unwrap();
    assert_eq!(s.scope_of(hub), Some(Major), "5 is not more than 5");
    add_citing(&mut s, Major, hub, 1);
    let res = s.query(subject_in(Enduring, "nothing")).unwrap();
    assert_eq!(res.query_stats.auto_promoted, 1);
    assert_eq!(s.scope_of(hub), Some(Enduring));
}

#[test]
fn references_from_removed_items_do_not_count() {
    let (_dir, mut s) = session_with(|c| c.promotion.major_to_enduring_references = 1);
    let hub = s.add(Major, fact("hub", "p", "x")).unwrap();
    add_citing(&mut s, Minor, hub, 2);
    let citing: Vec<_> = s
        .snapshot(Minor)
        .unwrap()
        .items
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    s.remove(Minor, &citing[0]).unwrap();
    s.query(subject_in(Enduring, "nothing")).unwrap();
    assert_eq!(s.scope_of(hub), Some(Major));
}

#[test]
fn reference_counts_do_not_promote_minor_items_directly() {
    let (_dir, mut s) = session_with(|c| c.promotion.major_to_enduring_references = 0);
    let a = s.add(Minor, fact("a", "p", "x")).unwrap();
    add_citing(&mut s, Minor, a, 3);
    s.query(subject_in(Enduring, "nothing")).unwrap();
    assert_eq!(s.scope_of(a), Some(Minor));
}

#[test]
fn an_item_moves_at_most_one_tier_per_query() {
    let (_dir, mut s) = session_with(|c| {
        c.promotion.minor_to_major_accesses = 0;
        c.promotion.major_to_enduring_references = 0;
    });
    let a = s.add(Minor, fact("a", "p", "x")).unwrap();
    add_citing(&mut s, Enduring, a, 1);
    s.query(subject_in(Minor, "a")).unwrap();
    assert_eq!(s.scope_of(a), Some(Major));
    s.query(subject_in(Enduring, "nothing")).unwrap();
    assert_eq!(s.scope_of(a), Some(Enduring));
}

// ---------------------------------------------------------------------------
// Cadence
// ---------------------------------------------------------------------------

#[test]
fn cadence_never_disables_auto_promotion() {
    let (_dir, mut s) = session_with(|c| {
        c.promotion.cadence = PromotionCadence::Never;
        c.promotion.major_to_enduring_references = 0;
    });
    let a = s.add(Minor, fact("a", "p", "b")).unwrap();
    let hub = s.add(Major, fact("hub", "p", "b")).unwrap();
    add_citing(&mut s, Minor, hub, 1);
    for _ in 0..10 {
        s.query(subject_in(Minor, "a")).unwrap();
    }
    assert_eq!(s.scope_of(a), Some(Minor));
    assert_eq!(s.scope_of(hub), Some(Major));
    // Explicit promotion still works.
    s.promote(&[a], Minor, Major).unwrap();
    assert_eq!(s.scope_of(a), Some(Major));
}
