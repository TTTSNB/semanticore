"""Demotion and eviction rules; mirrors crates/semanticore-context/tests/demotion.rs."""

from __future__ import annotations

import time

import pytest
from conftest import everything_in, fact, ids, subject_in

from semanticore_context import (
    ContextScope,
    DemotionTarget,
    DurabilityViolationError,
    EvictionCriteria,
    EvictionPolicy,
    ScopeMismatchError,
    Session,
    UnknownItemError,
)

ENDURING, MAJOR, MINOR = ContextScope.ENDURING, ContextScope.MAJOR, ContextScope.MINOR


# --- Explicit eviction: drop named items -----------------------------------


def test_evict_items_drops_them_and_returns_the_count(session):
    a = session.add(MAJOR, fact("a", "p", "b"))
    b = session.add(MAJOR, fact("b", "p", "c"))
    keep = session.add(MAJOR, fact("c", "p", "d"))
    assert session.evict(MAJOR, EvictionCriteria.items([a, b])) == 2
    assert session.scope_of(a) is None
    assert session.scope_of(b) is None
    assert ids(session, everything_in(MAJOR)) == [keep]
    assert session.close().items_evicted == 2


def test_evict_items_refuses_items_from_another_scope_and_removes_nothing(session):
    a = session.add(MAJOR, fact("a", "p", "b"))
    m = session.add(MINOR, fact("m", "p", "b"))
    with pytest.raises(ScopeMismatchError) as e:
        session.evict(MAJOR, EvictionCriteria.items([a, m]))
    assert (e.value.item_id, e.value.expected, e.value.found) == (m, MAJOR, MINOR)
    assert session.scope_of(a) == MAJOR


def test_evict_items_refuses_unknown_items(session):
    a = session.add(MAJOR, fact("a", "p", "b"))
    session.remove(MAJOR, a)
    with pytest.raises(UnknownItemError) as e:
        session.evict(MAJOR, EvictionCriteria.items([a]))
    assert e.value.item_id == a


# --- Explicit demotion -----------------------------------------------------


def test_demote_moves_items_to_a_colder_scope(session):
    a = session.add(ENDURING, fact("a", "p", "b"))
    assert session.evict(ENDURING, EvictionCriteria.demote([a], MINOR)) == 1
    assert session.scope_of(a) == MINOR
    assert ids(session, subject_in(MINOR, "a")) == [a]
    assert ids(session, subject_in(ENDURING, "a")) == []
    assert session.close().items_demoted == 1


def test_demote_refuses_a_target_that_is_not_colder(session):
    a = session.add(MAJOR, fact("a", "p", "b"))
    for to in (MAJOR, ENDURING):
        with pytest.raises(DurabilityViolationError) as e:
            session.evict(MAJOR, EvictionCriteria.demote([a], to))
        assert (e.value.from_scope, e.value.to_scope) == (MAJOR, to)
    assert session.scope_of(a) == MAJOR


def test_demoted_enduring_items_leave_the_disk(config):
    with Session.open(config) as s:
        a = s.add(ENDURING, fact("a", "p", "b"))
        s.evict(ENDURING, EvictionCriteria.demote([a], MAJOR))
    with Session.open(config) as s:
        assert s.item_count(ENDURING) == 0


def test_demotion_resets_the_minor_access_window(session):
    a = session.add(MINOR, fact("a", "p", "b"))
    for _ in range(4):
        session.query(subject_in(MINOR, "a"))
    assert session.scope_of(a) == MAJOR
    session.evict(MAJOR, EvictionCriteria.demote([a], MINOR))
    session.query(subject_in(MINOR, "a"))
    assert session.scope_of(a) == MINOR, "earlier accesses were forgotten"


# --- Policy-driven eviction: victim selection ------------------------------


def three_major(session_with, tweak=lambda c: None):
    s = session_with(tweak)
    a = s.add(MAJOR, fact("a", "p", "x"))
    b = s.add(MAJOR, fact("b", "p", "x"))
    c = s.add(MAJOR, fact("c", "p", "x"))
    return s, (a, b, c)


def policy(p):
    def tweak(c):
        c.eviction_policy = p

    return tweak


def test_lru_evicts_the_least_recently_touched_item(session_with):
    s, (a, b, c) = three_major(session_with, policy(EvictionPolicy.lru()))
    s.query(subject_in(MAJOR, "a"))
    s.query(subject_in(MAJOR, "c"))
    assert s.evict(MAJOR, EvictionCriteria.policy(1)) == 1
    assert s.scope_of(b) is None
    assert s.scope_of(a) == MAJOR
    assert s.scope_of(c) == MAJOR


def test_lfu_evicts_the_least_frequently_accessed_items(session_with):
    s, (a, b, c) = three_major(session_with, policy(EvictionPolicy.lfu()))
    for _ in range(2):
        s.query(subject_in(MAJOR, "a"))
    s.query(subject_in(MAJOR, "b"))
    # `c` was never accessed but is the newest: LFU still picks it first.
    assert s.evict(MAJOR, EvictionCriteria.policy(2)) == 2
    assert s.scope_of(c) is None
    assert s.scope_of(b) is None
    assert s.scope_of(a) == MAJOR


def test_lfu_breaks_ties_by_age(session_with):
    s, (a, b, c) = three_major(session_with, policy(EvictionPolicy.lfu()))
    s.evict(MAJOR, EvictionCriteria.policy(1))
    assert s.scope_of(a) is None
    assert s.scope_of(b) == MAJOR
    assert s.scope_of(c) == MAJOR


def test_recency_weighted_with_a_long_half_life_favours_frequency(session_with):
    s, (a, b, _) = three_major(session_with, policy(EvictionPolicy.recency_weighted(3600)))
    for _ in range(3):
        s.query(subject_in(MAJOR, "a"))
    s.query(subject_in(MAJOR, "b"))
    s.query(subject_in(MAJOR, "c"))
    s.query(subject_in(MAJOR, "c"))
    # LRU would evict `a`; with a long half-life `b` has the lowest weight.
    s.evict(MAJOR, EvictionCriteria.policy(1))
    assert s.scope_of(b) is None
    assert s.scope_of(a) == MAJOR


def test_recency_weighted_with_a_short_half_life_favours_recency(session_with):
    s, (a, b, _) = three_major(session_with, policy(EvictionPolicy.recency_weighted(0.001)))
    for _ in range(5):
        s.query(subject_in(MAJOR, "a"))
    time.sleep(0.06)
    s.query(subject_in(MAJOR, "b"))
    s.query(subject_in(MAJOR, "c"))
    # LFU would evict `b` or `c`; with a short half-life `a` has decayed.
    s.evict(MAJOR, EvictionCriteria.policy(1))
    assert s.scope_of(a) is None
    assert s.scope_of(b) == MAJOR


def test_policy_eviction_of_more_items_than_present_empties_the_scope(session_with):
    s, _ = three_major(session_with)
    assert s.evict(MAJOR, EvictionCriteria.policy(10)) == 3
    assert s.item_count(MAJOR) == 0


# --- Demotion target -------------------------------------------------------


def target(t):
    def tweak(c):
        c.demotion_target = t

    return tweak


def test_demotion_target_drop_discards_victims(session_with):
    s, (a, _, _) = three_major(session_with, target(DemotionTarget.DROP))
    s.evict(MAJOR, EvictionCriteria.policy(1))
    assert s.scope_of(a) is None
    assert s.item_count(MINOR) == 0


def test_demotion_target_next_colder_moves_victims_down_one_tier(session_with):
    s, (a, _, _) = three_major(session_with, target(DemotionTarget.NEXT_COLDER))
    assert s.evict(MAJOR, EvictionCriteria.policy(1)) == 1
    assert s.scope_of(a) == MINOR
    stats = s.close()
    assert (stats.items_demoted, stats.items_evicted) == (1, 0)


def test_demotion_out_of_minor_always_drops(session_with):
    s = session_with(target(DemotionTarget.NEXT_COLDER))
    a = s.add(MINOR, fact("a", "p", "b"))
    s.evict(MINOR, EvictionCriteria.policy(1))
    assert s.scope_of(a) is None


def test_one_step_then_drop_drops_items_that_were_added_directly(session_with):
    s, (a, _, _) = three_major(session_with)  # default target
    s.evict(MAJOR, EvictionCriteria.policy(1))
    assert s.scope_of(a) is None


def test_one_step_then_drop_demotes_promoted_items_once_then_drops_them(session):
    a = session.add(MAJOR, fact("a", "p", "b"))
    session.promote([a], MAJOR, ENDURING)
    # Recently promoted: one step colder rather than dropped.
    session.evict(ENDURING, EvictionCriteria.policy(1))
    assert session.scope_of(a) == MAJOR
    # Arrived by demotion: dropped rather than demoted again.
    session.evict(MAJOR, EvictionCriteria.policy(1))
    assert session.scope_of(a) is None


# --- Auto-demotion on ScopeBounds ------------------------------------------


def bounds(**kw):
    def tweak(c):
        for k, v in kw.items():
            setattr(c.bounds, k, v)

    return tweak


def test_exceeding_a_bound_on_add_evicts_down_to_the_bound(session_with):
    s = session_with(bounds(major_max_items=2))
    a = s.add(MAJOR, fact("a", "p", "x"))
    b = s.add(MAJOR, fact("b", "p", "x"))
    c = s.add(MAJOR, fact("c", "p", "x"))
    assert s.item_count(MAJOR) == 2
    assert s.scope_of(a) is None, "LRU victim, added directly: dropped"
    assert s.scope_of(b) == MAJOR
    assert s.scope_of(c) == MAJOR
    assert s.close().items_evicted == 1


def test_exceeding_a_bound_on_add_batch_evicts_after_the_batch(session_with):
    s = session_with(bounds(minor_max_items=1))
    got = s.add_batch(MINOR, [fact("a", "p", "x"), fact("b", "p", "x")])
    assert len(got) == 2
    assert s.item_count(MINOR) == 1
    assert s.scope_of(got[1]) == MINOR


def test_bounds_cascade_through_colder_tiers(session_with):
    def tweak(c):
        c.bounds.major_max_items = 1
        c.bounds.minor_max_items = 1
        c.demotion_target = DemotionTarget.NEXT_COLDER

    s = session_with(tweak)
    m1 = s.add(MAJOR, fact("m1", "p", "x"))
    m2 = s.add(MAJOR, fact("m2", "p", "x"))
    assert s.scope_of(m1) == MINOR
    m3 = s.add(MAJOR, fact("m3", "p", "x"))
    assert s.scope_of(m3) == MAJOR
    assert s.scope_of(m2) == MINOR
    assert s.scope_of(m1) is None


def test_promoting_into_a_full_tier_demotes_its_victim(session_with):
    s = session_with(bounds(enduring_max_items=1))
    e = s.add(ENDURING, fact("e", "p", "x"))
    m = s.add(MAJOR, fact("m", "p", "x"))
    s.promote([m], MAJOR, ENDURING)
    assert s.item_count(ENDURING) == 1
    assert s.scope_of(m) == ENDURING
    assert s.scope_of(e) is None


def test_unbounded_tiers_never_evict(session):
    for i in range(50):
        session.add(MINOR, fact(f"i{i}", "p", "x"))
    assert session.item_count(MINOR) == 50


def test_duplicate_ids_are_evicted_once(session):
    a = session.add(MAJOR, fact("a", "p", "b"))
    b = session.add(MAJOR, fact("b", "p", "b"))
    assert session.evict(MAJOR, EvictionCriteria.demote([a, a], MINOR)) == 1
    assert ids(session, subject_in(MINOR, "a")) == [a]
    assert session.evict(MAJOR, EvictionCriteria.items([b, b])) == 1
    assert session.scope_of(b) is None
