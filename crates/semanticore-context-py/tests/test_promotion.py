"""Promotion rules; mirrors crates/semanticore-context/tests/promotion.rs."""

from __future__ import annotations

import time

import pytest
from conftest import fact, ids, subject_in

from semanticore_context import (
    ContextScope,
    DurabilityViolationError,
    ItemId,
    PromotionCadence,
    Provenance,
    ScopeMismatchError,
    Session,
    UnknownItemError,
)

ENDURING, MAJOR, MINOR = ContextScope.ENDURING, ContextScope.MAJOR, ContextScope.MINOR


# --- ContextScope durability order ----------------------------------------


def test_scopes_are_ordered_by_durability():
    assert MINOR < MAJOR < ENDURING
    assert ContextScope.ALL == [ENDURING, MAJOR, MINOR]
    assert ENDURING.colder() == MAJOR
    assert MAJOR.colder() == MINOR
    assert MINOR.colder() is None


# --- Explicit promotion ----------------------------------------------------


def test_promote_moves_items_to_a_more_durable_scope(session):
    a = session.add(MINOR, fact("a", "p", "b"))
    b = session.add(MINOR, fact("a", "p", "c"))
    stats = session.promote([a, b], MINOR, MAJOR)
    assert stats.promoted == 2
    assert session.scope_of(a) == MAJOR
    assert session.scope_of(b) == MAJOR
    assert ids(session, subject_in(MAJOR, "a")) == [a, b]
    assert ids(session, subject_in(MINOR, "a")) == []
    assert session.close().items_promoted == 2


def test_promote_may_skip_a_tier(session):
    a = session.add(MINOR, fact("a", "p", "b"))
    session.promote([a], MINOR, ENDURING)
    assert session.scope_of(a) == ENDURING


def test_promote_within_the_same_scope_is_a_no_op(session):
    a = session.add(MAJOR, fact("a", "p", "b"))
    assert session.promote([a], MAJOR, MAJOR).promoted == 0
    assert session.scope_of(a) == MAJOR


def test_promote_refuses_a_less_durable_target(session):
    a = session.add(ENDURING, fact("a", "p", "b"))
    with pytest.raises(DurabilityViolationError) as e:
        session.promote([a], ENDURING, MINOR)
    assert (e.value.from_scope, e.value.to_scope) == (ENDURING, MINOR)
    assert session.scope_of(a) == ENDURING


def test_promote_refuses_items_not_in_the_source_scope_and_moves_nothing(session):
    a = session.add(MINOR, fact("a", "p", "b"))
    m = session.add(MAJOR, fact("m", "p", "b"))
    with pytest.raises(ScopeMismatchError) as e:
        session.promote([a, m], MINOR, ENDURING)
    assert (e.value.item_id, e.value.expected, e.value.found) == (m, MINOR, MAJOR)
    assert session.scope_of(a) == MINOR, "nothing moved"


def test_promote_refuses_unknown_items(session):
    a = session.add(MINOR, fact("a", "p", "b"))
    session.remove(MINOR, a)
    with pytest.raises(UnknownItemError) as e:
        session.promote([a], MINOR, MAJOR)
    assert e.value.item_id == a


def test_items_promoted_to_enduring_survive_reopen(config):
    with Session.open(config) as s:
        a = s.add(MAJOR, fact("a", "p", "b"))
        s.promote([a], MAJOR, ENDURING)
    with Session.open(config) as s:
        assert len(ids(s, subject_in(ENDURING, "a"))) == 1


# --- Auto-promotion: minor -> major on access frequency --------------------


def test_a_minor_item_accessed_more_than_the_threshold_auto_promotes_to_major(session):
    a = session.add(MINOR, fact("a", "p", "b"))
    # Default threshold is 3: three accesses are not "more than 3".
    for _ in range(3):
        assert ids(session, subject_in(MINOR, "a")) == [a]
        assert session.scope_of(a) == MINOR
    res = session.query(subject_in(MINOR, "a"))
    assert res.items[0].source_scope == MINOR, "served from minor"
    assert res.query_stats.auto_promoted == 1
    assert session.scope_of(a) == MAJOR
    assert ids(session, subject_in(MAJOR, "a")) == [a]
    assert session.close().items_promoted == 1


def test_only_returned_items_count_as_accessed(session):
    a = session.add(MINOR, fact("a", "p", "b"))
    b = session.add(MINOR, fact("b", "p", "c"))
    for _ in range(4):
        session.query(subject_in(MINOR, "a"))
    assert session.scope_of(a) == MAJOR
    assert session.scope_of(b) == MINOR


def test_items_cut_by_the_limit_are_not_accessed(session):
    a = session.add(MINOR, fact("x", "p", "a"))
    b = session.add(MINOR, fact("x", "p", "b"))
    for _ in range(4):
        session.query(subject_in(MINOR, "x").with_limit(1))
    assert session.scope_of(a) == MAJOR
    assert session.scope_of(b) == MINOR


def test_accesses_outside_the_window_do_not_count(session_with):
    def tweak(c):
        c.promotion.minor_window = 0.001

    s = session_with(tweak)
    a = s.add(MINOR, fact("a", "p", "b"))
    for _ in range(6):
        s.query(subject_in(MINOR, "a"))
        time.sleep(0.005)
    assert s.scope_of(a) == MINOR


def test_the_access_threshold_is_configurable(session_with):
    def tweak(c):
        c.promotion.minor_to_major_accesses = 0

    s = session_with(tweak)
    a = s.add(MINOR, fact("a", "p", "b"))
    s.query(subject_in(MINOR, "a"))
    assert s.scope_of(a) == MAJOR


def test_accesses_in_major_do_not_auto_promote_to_enduring(session):
    a = session.add(MAJOR, fact("a", "p", "b"))
    for _ in range(10):
        session.query(subject_in(MAJOR, "a"))
    assert session.scope_of(a) == MAJOR


# --- Auto-promotion: major -> enduring on reference count ------------------


def add_citing(s: Session, scope: ContextScope, cited: ItemId, n: int) -> None:
    for i in range(n):
        item = fact(f"cite{i}", "p", "x").with_provenance(Provenance("test", [cited]))
        s.add(scope, item)


def test_a_major_item_referenced_by_more_than_the_threshold_auto_promotes(session):
    hub = session.add(MAJOR, fact("hub", "p", "x"))
    add_citing(session, MINOR, hub, 5)
    # Promotion runs at the end of a query, whatever the query returned.
    session.query(subject_in(ENDURING, "nothing"))
    assert session.scope_of(hub) == MAJOR, "5 is not more than 5"
    add_citing(session, MAJOR, hub, 1)
    res = session.query(subject_in(ENDURING, "nothing"))
    assert res.query_stats.auto_promoted == 1
    assert session.scope_of(hub) == ENDURING


def test_references_from_removed_items_do_not_count(session_with):
    def tweak(c):
        c.promotion.major_to_enduring_references = 1

    s = session_with(tweak)
    hub = s.add(MAJOR, fact("hub", "p", "x"))
    add_citing(s, MINOR, hub, 2)
    citing = [i for i, _ in s.snapshot(MINOR).items]
    s.remove(MINOR, citing[0])
    s.query(subject_in(ENDURING, "nothing"))
    assert s.scope_of(hub) == MAJOR


def test_reference_counts_do_not_promote_minor_items_directly(session_with):
    def tweak(c):
        c.promotion.major_to_enduring_references = 0

    s = session_with(tweak)
    a = s.add(MINOR, fact("a", "p", "x"))
    add_citing(s, MINOR, a, 3)
    s.query(subject_in(ENDURING, "nothing"))
    assert s.scope_of(a) == MINOR


def test_an_item_moves_at_most_one_tier_per_query(session_with):
    def tweak(c):
        c.promotion.minor_to_major_accesses = 0
        c.promotion.major_to_enduring_references = 0

    s = session_with(tweak)
    a = s.add(MINOR, fact("a", "p", "x"))
    add_citing(s, ENDURING, a, 1)
    s.query(subject_in(MINOR, "a"))
    assert s.scope_of(a) == MAJOR
    s.query(subject_in(ENDURING, "nothing"))
    assert s.scope_of(a) == ENDURING


# --- Cadence ---------------------------------------------------------------


def test_cadence_never_disables_auto_promotion(session_with):
    def tweak(c):
        c.promotion.cadence = PromotionCadence.NEVER
        c.promotion.major_to_enduring_references = 0

    s = session_with(tweak)
    a = s.add(MINOR, fact("a", "p", "b"))
    hub = s.add(MAJOR, fact("hub", "p", "b"))
    add_citing(s, MINOR, hub, 1)
    for _ in range(10):
        s.query(subject_in(MINOR, "a"))
    assert s.scope_of(a) == MINOR
    assert s.scope_of(hub) == MAJOR
    # Explicit promotion still works.
    s.promote([a], MINOR, MAJOR)
    assert s.scope_of(a) == MAJOR


def test_duplicate_ids_are_moved_once(session):
    a = session.add(MINOR, fact("a", "p", "b"))
    assert session.promote([a, a], MINOR, MAJOR).promoted == 1
    assert session.scope_of(a) == MAJOR
    assert ids(session, subject_in(MAJOR, "a")) == [a]
