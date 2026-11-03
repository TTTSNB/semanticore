//! Demotion and eviction rules (RFC 0004 "Promotion / demotion semantics"
//! and "Eviction"): `Session::evict`, `ScopeBounds`, `EvictionPolicy` and
//! `DemotionTarget`.

mod common;

use std::time::Duration;

use common::*;
use semanticore_context::{
    ContextScope, DemotionTarget, EvictionCriteria, EvictionPolicy, Session, SessionError,
};

use ContextScope::{Enduring, Major, Minor};

// ---------------------------------------------------------------------------
// Explicit eviction: drop named items
// ---------------------------------------------------------------------------

#[test]
fn evict_items_drops_them_and_returns_the_count() {
    let (_dir, mut s) = session();
    let a = s.add(Major, fact("a", "p", "b")).unwrap();
    let b = s.add(Major, fact("b", "p", "c")).unwrap();
    let keep = s.add(Major, fact("c", "p", "d")).unwrap();
    assert_eq!(s.evict(Major, EvictionCriteria::Items(vec![a, b])).unwrap(), 2);
    assert_eq!(s.scope_of(a), None);
    assert_eq!(s.scope_of(b), None);
    assert_eq!(ids(&s, everything_in(Major)), vec![keep]);
    assert_eq!(s.close().unwrap().items_evicted, 2);
}

#[test]
fn evict_items_refuses_items_from_another_scope_and_removes_nothing() {
    let (_dir, mut s) = session();
    let a = s.add(Major, fact("a", "p", "b")).unwrap();
    let m = s.add(Minor, fact("m", "p", "b")).unwrap();
    assert!(matches!(
        s.evict(Major, EvictionCriteria::Items(vec![a, m])),
        Err(SessionError::ScopeMismatch { item, expected: Major, found: Minor }) if item == m
    ));
    assert_eq!(s.scope_of(a), Some(Major));
}

#[test]
fn evict_items_refuses_unknown_items() {
    let (_dir, mut s) = session();
    let a = s.add(Major, fact("a", "p", "b")).unwrap();
    s.remove(Major, &a).unwrap();
    assert!(matches!(
        s.evict(Major, EvictionCriteria::Items(vec![a])),
        Err(SessionError::UnknownItem(id)) if id == a
    ));
}

// ---------------------------------------------------------------------------
// Explicit demotion
// ---------------------------------------------------------------------------

#[test]
fn demote_moves_items_to_a_colder_scope() {
    let (_dir, mut s) = session();
    let a = s.add(Enduring, fact("a", "p", "b")).unwrap();
    let n = s
        .evict(
            Enduring,
            EvictionCriteria::Demote {
                items: vec![a],
                to: Minor,
            },
        )
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(s.scope_of(a), Some(Minor));
    assert_eq!(ids(&s, subject_in(Minor, "a")), vec![a]);
    assert!(ids(&s, subject_in(Enduring, "a")).is_empty());
    assert_eq!(s.close().unwrap().items_demoted, 1);
}

#[test]
fn demote_refuses_a_target_that_is_not_colder() {
    let (_dir, mut s) = session();
    let a = s.add(Major, fact("a", "p", "b")).unwrap();
    for to in [Major, Enduring] {
        match s.evict(
            Major,
            EvictionCriteria::Demote {
                items: vec![a],
                to,
            },
        ) {
            Err(SessionError::DurabilityViolation { from, to: got }) => {
                assert_eq!((from, got), (Major, to));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(s.scope_of(a), Some(Major));
}

#[test]
fn demoted_enduring_items_leave_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut s = Session::open(config(dir.path())).unwrap();
        let a = s.add(Enduring, fact("a", "p", "b")).unwrap();
        s.evict(
            Enduring,
            EvictionCriteria::Demote {
                items: vec![a],
                to: Major,
            },
        )
        .unwrap();
        s.close().unwrap();
    }
    let s = Session::open(config(dir.path())).unwrap();
    assert_eq!(s.item_count(Enduring), 0);
}

#[test]
fn demotion_resets_the_minor_access_window() {
    let (_dir, mut s) = session();
    let a = s.add(Minor, fact("a", "p", "b")).unwrap();
    for _ in 0..4 {
        s.query(subject_in(Minor, "a")).unwrap();
    }
    assert_eq!(s.scope_of(a), Some(Major));
    s.evict(
        Major,
        EvictionCriteria::Demote {
            items: vec![a],
            to: Minor,
        },
    )
    .unwrap();
    s.query(subject_in(Minor, "a")).unwrap();
    assert_eq!(s.scope_of(a), Some(Minor), "earlier accesses were forgotten");
}

// ---------------------------------------------------------------------------
// Policy-driven eviction: victim selection
// ---------------------------------------------------------------------------

fn three_major(
    tweak: impl FnOnce(&mut semanticore_context::SessionConfig<semanticore_store_oxigraph::OxigraphStore>),
) -> (
    tempfile::TempDir,
    Session<semanticore_store_oxigraph::OxigraphStore>,
    [semanticore_context::ItemId; 3],
) {
    let (dir, mut s) = session_with(tweak);
    let a = s.add(Major, fact("a", "p", "x")).unwrap();
    let b = s.add(Major, fact("b", "p", "x")).unwrap();
    let c = s.add(Major, fact("c", "p", "x")).unwrap();
    (dir, s, [a, b, c])
}

#[test]
fn lru_evicts_the_least_recently_touched_item() {
    let (_dir, mut s, [a, b, c]) = three_major(|c| c.eviction_policy = EvictionPolicy::Lru);
    s.query(subject_in(Major, "a")).unwrap();
    s.query(subject_in(Major, "c")).unwrap();
    assert_eq!(s.evict(Major, EvictionCriteria::Policy { count: 1 }).unwrap(), 1);
    assert_eq!(s.scope_of(b), None);
    assert_eq!(s.scope_of(a), Some(Major));
    assert_eq!(s.scope_of(c), Some(Major));
}

#[test]
fn lfu_evicts_the_least_frequently_accessed_items() {
    let (_dir, mut s, [a, b, c]) = three_major(|c| c.eviction_policy = EvictionPolicy::Lfu);
    for _ in 0..2 {
        s.query(subject_in(Major, "a")).unwrap();
    }
    s.query(subject_in(Major, "b")).unwrap();
    // `c` was never accessed but is the newest: LFU still picks it first.
    assert_eq!(s.evict(Major, EvictionCriteria::Policy { count: 2 }).unwrap(), 2);
    assert_eq!(s.scope_of(c), None);
    assert_eq!(s.scope_of(b), None);
    assert_eq!(s.scope_of(a), Some(Major));
}

#[test]
fn lfu_breaks_ties_by_age() {
    let (_dir, mut s, [a, b, c]) = three_major(|c| c.eviction_policy = EvictionPolicy::Lfu);
    s.evict(Major, EvictionCriteria::Policy { count: 1 }).unwrap();
    assert_eq!(s.scope_of(a), None);
    assert_eq!(s.scope_of(b), Some(Major));
    assert_eq!(s.scope_of(c), Some(Major));
}

#[test]
fn recency_weighted_with_a_long_half_life_favours_frequency() {
    let (_dir, mut s, [a, b, _]) = three_major(|c| {
        c.eviction_policy = EvictionPolicy::RecencyWeighted {
            half_life: Duration::from_secs(3600),
        }
    });
    for _ in 0..3 {
        s.query(subject_in(Major, "a")).unwrap();
    }
    s.query(subject_in(Major, "b")).unwrap();
    s.query(subject_in(Major, "c")).unwrap();
    s.query(subject_in(Major, "c")).unwrap();
    // LRU would evict `a`; with a long half-life `b` has the lowest weight.
    s.evict(Major, EvictionCriteria::Policy { count: 1 }).unwrap();
    assert_eq!(s.scope_of(b), None);
    assert_eq!(s.scope_of(a), Some(Major));
}

#[test]
fn recency_weighted_with_a_short_half_life_favours_recency() {
    let (_dir, mut s, [a, b, _]) = three_major(|c| {
        c.eviction_policy = EvictionPolicy::RecencyWeighted {
            half_life: Duration::from_millis(1),
        }
    });
    for _ in 0..5 {
        s.query(subject_in(Major, "a")).unwrap();
    }
    std::thread::sleep(Duration::from_millis(60));
    s.query(subject_in(Major, "b")).unwrap();
    s.query(subject_in(Major, "c")).unwrap();
    // LFU would evict `b` or `c`; with a short half-life `a` has decayed.
    s.evict(Major, EvictionCriteria::Policy { count: 1 }).unwrap();
    assert_eq!(s.scope_of(a), None);
    assert_eq!(s.scope_of(b), Some(Major));
}

#[test]
fn policy_eviction_of_more_items_than_present_empties_the_scope() {
    let (_dir, mut s, _) = three_major(|_| {});
    assert_eq!(s.evict(Major, EvictionCriteria::Policy { count: 10 }).unwrap(), 3);
    assert_eq!(s.item_count(Major), 0);
}

// ---------------------------------------------------------------------------
// Demotion target
// ---------------------------------------------------------------------------

#[test]
fn demotion_target_drop_discards_victims() {
    let (_dir, mut s, [a, ..]) = three_major(|c| c.demotion_target = DemotionTarget::Drop);
    s.evict(Major, EvictionCriteria::Policy { count: 1 }).unwrap();
    assert_eq!(s.scope_of(a), None);
    assert_eq!(s.item_count(Minor), 0);
}

#[test]
fn demotion_target_next_colder_moves_victims_down_one_tier() {
    let (_dir, mut s, [a, ..]) = three_major(|c| c.demotion_target = DemotionTarget::NextColder);
    assert_eq!(s.evict(Major, EvictionCriteria::Policy { count: 1 }).unwrap(), 1);
    assert_eq!(s.scope_of(a), Some(Minor));
    let stats = s.close().unwrap();
    assert_eq!((stats.items_demoted, stats.items_evicted), (1, 0));
}

#[test]
fn demotion_out_of_minor_always_drops() {
    let (_dir, mut s) = session_with(|c| c.demotion_target = DemotionTarget::NextColder);
    let a = s.add(Minor, fact("a", "p", "b")).unwrap();
    s.evict(Minor, EvictionCriteria::Policy { count: 1 }).unwrap();
    assert_eq!(s.scope_of(a), None);
}

#[test]
fn one_step_then_drop_drops_items_that_were_added_directly() {
    let (_dir, mut s, [a, ..]) = three_major(|_| {}); // default target
    s.evict(Major, EvictionCriteria::Policy { count: 1 }).unwrap();
    assert_eq!(s.scope_of(a), None);
}

#[test]
fn one_step_then_drop_demotes_promoted_items_once_then_drops_them() {
    let (_dir, mut s) = session(); // default target
    let a = s.add(Major, fact("a", "p", "b")).unwrap();
    s.promote(&[a], Major, Enduring).unwrap();
    // Recently promoted: one step colder rather than dropped.
    s.evict(Enduring, EvictionCriteria::Policy { count: 1 }).unwrap();
    assert_eq!(s.scope_of(a), Some(Major));
    // Arrived by demotion: dropped rather than demoted again.
    s.evict(Major, EvictionCriteria::Policy { count: 1 }).unwrap();
    assert_eq!(s.scope_of(a), None);
}

// ---------------------------------------------------------------------------
// Auto-demotion on ScopeBounds
// ---------------------------------------------------------------------------

#[test]
fn exceeding_a_bound_on_add_evicts_down_to_the_bound() {
    let (_dir, mut s) = session_with(|c| c.bounds.major_max_items = Some(2));
    let a = s.add(Major, fact("a", "p", "x")).unwrap();
    let b = s.add(Major, fact("b", "p", "x")).unwrap();
    let c = s.add(Major, fact("c", "p", "x")).unwrap();
    assert_eq!(s.item_count(Major), 2);
    assert_eq!(s.scope_of(a), None, "LRU victim, added directly: dropped");
    assert_eq!(s.scope_of(b), Some(Major));
    assert_eq!(s.scope_of(c), Some(Major));
    assert_eq!(s.close().unwrap().items_evicted, 1);
}

#[test]
fn exceeding_a_bound_on_add_batch_evicts_after_the_batch() {
    let (_dir, mut s) = session_with(|c| c.bounds.minor_max_items = Some(1));
    let ids = s
        .add_batch(Minor, &[fact("a", "p", "x"), fact("b", "p", "x")])
        .unwrap();
    assert_eq!(ids.len(), 2);
    assert_eq!(s.item_count(Minor), 1);
    assert_eq!(s.scope_of(ids[1]), Some(Minor));
}

#[test]
fn bounds_cascade_through_colder_tiers() {
    let (_dir, mut s) = session_with(|c| {
        c.bounds.major_max_items = Some(1);
        c.bounds.minor_max_items = Some(1);
        c.demotion_target = DemotionTarget::NextColder;
    });
    let m1 = s.add(Major, fact("m1", "p", "x")).unwrap();
    let m2 = s.add(Major, fact("m2", "p", "x")).unwrap();
    assert_eq!(s.scope_of(m1), Some(Minor));
    let m3 = s.add(Major, fact("m3", "p", "x")).unwrap();
    assert_eq!(s.scope_of(m3), Some(Major));
    assert_eq!(s.scope_of(m2), Some(Minor));
    assert_eq!(s.scope_of(m1), None);
}

#[test]
fn promoting_into_a_full_tier_demotes_its_victim() {
    let (_dir, mut s) = session_with(|c| c.bounds.enduring_max_items = Some(1));
    let e = s.add(Enduring, fact("e", "p", "x")).unwrap();
    let m = s.add(Major, fact("m", "p", "x")).unwrap();
    s.promote(&[m], Major, Enduring).unwrap();
    assert_eq!(s.item_count(Enduring), 1);
    assert_eq!(s.scope_of(m), Some(Enduring));
    assert_eq!(s.scope_of(e), None);
}

#[test]
fn unbounded_tiers_never_evict() {
    let (_dir, mut s) = session();
    for i in 0..50 {
        s.add(Minor, fact(&format!("i{i}"), "p", "x")).unwrap();
    }
    assert_eq!(s.item_count(Minor), 50);
}
