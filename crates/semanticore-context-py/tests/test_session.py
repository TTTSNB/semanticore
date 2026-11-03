"""Session lifecycle, config, items, the reserved surfaces and the error
hierarchy; mirrors crates/semanticore-context/tests/{session,items,reserved}.rs
plus binding-specific behaviour (context manager, closed sessions, GIL)."""

from __future__ import annotations

import threading
from datetime import timedelta

import pytest
from conftest import everything_in, ex, fact, ids, prov, subject_in, triple

import semanticore_context as sc
from semanticore_context import (
    BlankNode,
    ContextItem,
    ContextScope,
    DemotionTarget,
    EvictionPolicy,
    Literal,
    PromotionCadence,
    PromotionConfig,
    Provenance,
    ScopeBounds,
    Session,
    SessionConfig,
)

ENDURING, MAJOR, MINOR = ContextScope.ENDURING, ContextScope.MAJOR, ContextScope.MINOR


# --- SessionConfig ---------------------------------------------------------


def test_session_config_defaults_follow_the_rfc(config):
    assert config.bounds == ScopeBounds()
    assert config.bounds.major_max_items is None
    assert config.eviction_policy == EvictionPolicy.lru()
    assert config.demotion_target == DemotionTarget.ONE_STEP_THEN_DROP
    p = config.promotion
    assert p.cadence == PromotionCadence.AFTER_EACH_QUERY
    assert p.minor_to_major_accesses == 3
    assert p.minor_window == 60.0
    assert p.major_to_enduring_references == 5


def test_session_config_takes_settings_as_keywords(tmp_path):
    cfg = SessionConfig.oxigraph(
        tmp_path,
        bounds=ScopeBounds(minor_max_items=4),
        eviction_policy=EvictionPolicy.recency_weighted(timedelta(seconds=2)),
        demotion_target=DemotionTarget.DROP,
        promotion=PromotionConfig(minor_window=timedelta(minutes=2)),
    )
    assert cfg.bounds.minor_max_items == 4
    assert cfg.eviction_policy.kind == "recency_weighted"
    assert cfg.eviction_policy.half_life == 2.0
    assert cfg.demotion_target == DemotionTarget.DROP
    assert cfg.promotion.minor_window == 120.0
    assert cfg.enduring_path == tmp_path


def test_nested_settings_are_live(config):
    config.bounds.major_max_items = 2
    config.promotion.minor_to_major_accesses = 0
    assert config.bounds.major_max_items == 2
    assert config.promotion.minor_to_major_accesses == 0


def test_invalid_durations_are_refused():
    with pytest.raises(ValueError):
        PromotionConfig(minor_window=-1)
    with pytest.raises(TypeError):
        EvictionPolicy.recency_weighted("1s")


def test_a_config_can_open_several_sessions_in_turn(config):
    with Session.open(config) as s:
        s.add(ENDURING, fact("a", "p", "b"))
    with Session.open(config) as s:
        assert s.item_count(ENDURING) == 1


def test_a_locked_enduring_store_raises_storage_error(config):
    with Session.open(config):
        with pytest.raises(sc.StorageError):
            Session.open(config)


# --- Lifecycle -------------------------------------------------------------


def test_close_reports_session_stats(session):
    session.add(MINOR, fact("a", "p", "b"))
    session.add_batch(MAJOR, [fact("c", "p", "d"), fact("e", "p", "f")])
    session.query(everything_in(MAJOR))
    session.query(everything_in(MINOR))
    stats = session.close()
    assert stats.items_added == 3
    assert stats.queries_served == 2
    assert stats.reasoning_passes == 0
    assert stats.items_promoted == 0
    assert stats.items_demoted == 0
    assert stats.items_evicted == 0


def test_context_manager_closes_even_on_exception(config):
    with pytest.raises(RuntimeError):
        with Session.open(config) as s:
            raise RuntimeError("boom")
    assert s.closed


def test_close_is_idempotent_and_later_calls_raise(session):
    session.add(MINOR, fact("a", "p", "b"))
    first = session.close()
    assert session.closed
    assert session.close() == first
    with pytest.raises(sc.SessionClosedError):
        session.add(MINOR, fact("a", "p", "b"))
    with pytest.raises(sc.SessionError):
        session.query(everything_in(MINOR))


def test_enduring_items_survive_close_and_reopen(config):
    event = ContextItem.event(
        "tool_call", {"tool": "search", "n": 3}, prov(), embedding=[0.25, -1.5]
    )
    chunk = ContextItem.chunk("hello", "text/plain", prov(), scope_hint=MAJOR)
    with Session.open(config) as s:
        fact_id = s.add(ENDURING, fact("a", "p", "b"))
        event_id = s.add(ENDURING, event)
        chunk_id = s.add(ENDURING, chunk)
        s.add(MAJOR, fact("major", "p", "x"))
        s.add(MINOR, fact("minor", "p", "x"))

    with Session.open(config) as s:
        assert s.item_count(ENDURING) == 3
        assert s.item_count(MAJOR) == 0, "major is in-memory"
        assert s.item_count(MINOR) == 0, "minor is in-memory"
        items = dict(s.snapshot(ENDURING).items)
        assert items[fact_id] == fact("a", "p", "b")
        assert items[event_id] == event
        assert items[chunk_id] == chunk
        assert ids(s, subject_in(ENDURING, "a")) == [fact_id]


def test_snapshot_lists_items_in_id_order_with_the_tier_quad_count(session):
    a = session.add(MAJOR, fact("a", "p", "b"))
    b = session.add(
        MAJOR,
        ContextItem.from_triples([triple("c", "p", ex("d")), triple("c", "q", ex("e"))], prov()),
    )
    session.add(MINOR, fact("x", "p", "y"))
    snap = session.snapshot(MAJOR)
    assert snap.scope == MAJOR
    assert [i for i, _ in snap.items] == [a, b]
    # Three payload quads plus one metadata record per item.
    assert snap.quad_count == 5


def test_remove_returns_whether_the_item_was_in_the_scope(session):
    a = session.add(MAJOR, fact("a", "p", "b"))
    assert not session.remove(MINOR, a)
    assert session.remove(MAJOR, a)
    assert not session.remove(MAJOR, a)
    assert session.scope_of(a) is None


def test_concurrent_queries_from_python_threads(session):
    for i in range(20):
        session.add(MAJOR, fact(f"s{i}", "p", "o"))
    errors: list[BaseException] = []

    def worker():
        try:
            for _ in range(20):
                assert len(session.query(everything_in(MAJOR)).items) == 20
        except BaseException as e:  # noqa: BLE001 - surfaced below
            errors.append(e)

    threads = [threading.Thread(target=worker) for _ in range(4)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert errors == []


# --- Items -----------------------------------------------------------------


def test_items_round_trip_their_payloads():
    body = {"tool": "search", "args": [1, 2.5, True, None], "nested": {"k": "v"}}
    e = ContextItem.event("tool_call", body, "agent")
    assert e.payload_type == "event"
    assert e.event_kind == "tool_call"
    assert e.event_body == body
    assert e.triples is None and e.content is None

    c = ContextItem.chunk("hi", "text/plain", Provenance("doc"), scope_hint=MINOR)
    assert (c.payload_type, c.content, c.content_type) == ("chunk", "hi", "text/plain")
    assert c.scope_hint == MINOR
    assert c.provenance.source == "doc"

    t = ContextItem.from_triples(
        [(BlankNode("b0"), ex("name"), Literal("Bob", language="en"))], "src", embedding=[0.5]
    )
    assert t.payload_type == "triples"
    assert t.triples == [(BlankNode("b0"), ex("name"), Literal("Bob", language="en"))]
    assert t.embedding == [0.5]


def test_items_are_values():
    assert fact("a", "p", "b") == fact("a", "p", "b")
    assert fact("a", "p", "b") != fact("a", "p", "c")
    with pytest.raises(AttributeError):
        fact("a", "p", "b").provenance = prov()  # type: ignore[misc]


def test_literals_and_blank_nodes():
    assert Literal("x").datatype == "http://www.w3.org/2001/XMLSchema#string"
    assert Literal("x", language="en").language == "en"
    assert Literal("3", datatype="urn:int").datatype == "urn:int"
    with pytest.raises(ValueError):
        Literal("x", datatype="urn:dt", language="en")
    assert BlankNode("b").id == "b"
    assert {Literal("x"), Literal("x"), BlankNode("x")} == {Literal("x"), BlankNode("x")}


def test_items_without_provenance_are_refused(session):
    with pytest.raises(sc.MissingProvenanceError):
        session.add(MAJOR, ContextItem.chunk("x", "text/plain", ""))


def test_non_finite_embeddings_are_refused(session):
    with pytest.raises(sc.InvalidItemError) as e:
        session.add(MAJOR, ContextItem.chunk("x", "text/plain", prov(), embedding=[float("nan")]))
    assert "finite" in e.value.reason


def test_derived_from_unknown_items_is_refused(session):
    a = session.add(MINOR, fact("a", "p", "b"))
    session.remove(MINOR, a)
    with pytest.raises(sc.UnknownItemError) as e:
        session.add(MINOR, fact("c", "p", "d").with_provenance(Provenance("t", [a])))
    assert e.value.item_id == a


def test_add_batch_is_all_or_nothing(session):
    with pytest.raises(sc.MissingProvenanceError):
        session.add_batch(
            MAJOR, [fact("a", "p", "b"), ContextItem.chunk("x", "text/plain", " ")]
        )
    assert session.item_count(MAJOR) == 0


def test_bad_payloads_raise_python_errors():
    with pytest.raises(TypeError):
        ContextItem.from_triples([["urn:s", "urn:p", "urn:o"]], "src")
    with pytest.raises(ValueError):
        ContextItem.from_triples([("urn:s", "urn:p")], "src")
    with pytest.raises(TypeError):
        ContextItem.event("k", {1: "non-str key"}, "src")
    with pytest.raises(ValueError):
        ContextItem.event("k", float("inf"), "src")
    with pytest.raises(TypeError):
        ContextItem.chunk("x", "text/plain", 42)


def test_item_ids_are_hashable_and_ordered(session):
    a = session.add(MINOR, fact("a", "p", "b"))
    b = session.add(MINOR, fact("b", "p", "c"))
    assert a < b and len({a, b, a}) == 2
    assert str(a).startswith("item:")
    with pytest.raises(TypeError):
        sc.ItemId()  # type: ignore[call-arg]


# --- Reserved surfaces (premium-only / not yet implemented) ----------------


def test_validate_is_premium_only(session):
    with pytest.raises(sc.ShaclUnavailableError) as e:
        session.validate(ENDURING, MAJOR)
    assert isinstance(e.value, sc.PremiumOnlyError)
    assert "premium" in str(e.value)


def test_datalog_is_premium_only(session):
    with pytest.raises(sc.DatalogUnavailableError) as e:
        session.evaluate_datalog(ENDURING)
    assert isinstance(e.value, sc.PremiumOnlyError)
    assert "premium" in str(e.value)


def test_reason_is_not_yet_implemented_for_every_scope(session):
    for scope in ContextScope.ALL:
        with pytest.raises(sc.NotYetImplementedError) as e:
            session.reason(scope)
        assert e.value.feature == "owl_el_reasoner"
        assert "not yet implemented" in str(e.value)
    assert session.close().reasoning_passes == 0


def test_not_yet_implemented_and_premium_only_are_distinct():
    assert not issubclass(sc.NotYetImplementedError, sc.PremiumOnlyError)
    assert not issubclass(sc.PremiumOnlyError, sc.NotYetImplementedError)


@pytest.mark.parametrize(
    "name",
    [
        "CapabilityShortfallError",
        "MissingProvenanceError",
        "InvalidItemError",
        "UnknownItemError",
        "ScopeMismatchError",
        "DurabilityViolationError",
        "PremiumOnlyError",
        "ShaclUnavailableError",
        "DatalogUnavailableError",
        "NotYetImplementedError",
        "CorruptStoreError",
        "StorageError",
        "SessionClosedError",
    ],
)
def test_every_error_is_a_session_error(name):
    cls = getattr(sc, name)
    assert issubclass(cls, sc.SessionError)
    assert issubclass(sc.SessionError, Exception)


def test_scope_enum_is_pythonic():
    assert str(MAJOR) == "major"
    assert repr(MAJOR) == "ContextScope.MAJOR"
    assert sorted([ENDURING, MINOR, MAJOR]) == [MINOR, MAJOR, ENDURING]
    assert len({MAJOR, MAJOR}) == 1


def test_module_has_a_version():
    assert sc.__version__ == "0.1.0"
