"""ContextQuery, ContextQueryResult and Session.query; mirrors
crates/semanticore-context/tests/query.rs."""

from __future__ import annotations

import pytest
from conftest import ex, everything_in, fact, ids, prov, subject_in, triple

from semanticore_context import (
    ContextItem,
    ContextQuery,
    ContextQueryResult,
    ContextScope,
    Literal,
    NotYetImplementedError,
    SessionError,
)

ENDURING, MAJOR, MINOR = ContextScope.ENDURING, ContextScope.MAJOR, ContextScope.MINOR


def test_context_query_builders_fill_the_rfc_fields():
    q = ContextQuery.pattern()
    assert q.kind == "pattern"
    assert q.scopes is None  # all scopes
    assert q.limit is None
    assert q.reasoner_hint is None

    q = ContextQuery.sparql("ASK {}", scopes=MAJOR).with_limit(3).with_reasoner_hint(MAJOR)
    assert q.kind == "sparql"
    assert q.scopes == MAJOR
    assert q.limit == 3
    assert q.reasoner_hint == MAJOR

    q = ContextQuery.pattern(ex("s"), ex("p"), ex("o"), scopes=[MINOR, ENDURING], limit=2)
    assert q.scopes == [MINOR, ENDURING]
    assert q.limit == 2
    assert q == ContextQuery.pattern(ex("s"), ex("p"), ex("o"), scopes=[MINOR, ENDURING], limit=2)

    assert ContextQuery.similarity([1.0], 3).kind == "similarity"
    assert ContextQuery.inference(ex("s")).kind == "inference"


def test_with_methods_return_copies():
    q = ContextQuery.pattern()
    q2 = q.with_limit(1)
    assert q.limit is None and q2.limit == 1


def test_pattern_query_returns_matching_items_with_score_one(session):
    alice = session.add(MAJOR, fact("alice", "knows", "bob"))
    session.add(MAJOR, fact("carol", "knows", "bob"))
    res = session.query(subject_in(MAJOR, "alice"))
    assert isinstance(res, ContextQueryResult)
    assert len(res.items) == 1
    hit = res.items[0]
    assert hit.id == alice
    assert hit.score == 1.0
    assert hit.source_scope == MAJOR
    assert hit.item == fact("alice", "knows", "bob")


def test_pattern_matches_on_predicate_and_object(session):
    a = session.add(MINOR, fact("a", "likes", "tea"))
    lit = ContextItem.from_triples([triple("b", "name", Literal("B"))], prov())
    b = session.add(MINOR, lit)
    assert ids(session, ContextQuery.pattern(predicate=ex("likes"), scopes=MINOR)) == [a]
    assert ids(session, ContextQuery.pattern(object=Literal("B"), scopes=MINOR)) == [b]


def test_an_item_matching_several_quads_is_returned_once(session):
    item = ContextItem.from_triples([triple("a", "p", ex("b")), triple("a", "q", ex("c"))], prov())
    i = session.add(MAJOR, item)
    assert ids(session, subject_in(MAJOR, "a")) == [i]


def test_events_and_chunks_never_match_patterns(session):
    session.add(MAJOR, ContextItem.event("turn", {}, prov()))
    session.add(MAJOR, ContextItem.chunk("hi", "text/plain", prov()))
    assert ids(session, everything_in(MAJOR)) == []


def test_scope_selection_controls_which_tiers_are_consulted(session):
    e = session.add(ENDURING, fact("x", "p", "e"))
    ma = session.add(MAJOR, fact("x", "p", "m"))
    mi = session.add(MINOR, fact("x", "p", "n"))

    assert ids(session, ContextQuery.pattern(ex("x"), scopes=MAJOR)) == [ma]

    # The default (all scopes) merges in durability order.
    res = session.query(ContextQuery.pattern(ex("x")))
    assert [(r.id, r.source_scope) for r in res.items] == [
        (e, ENDURING),
        (ma, MAJOR),
        (mi, MINOR),
    ]

    # A list consults the named scopes, in the order given, once each.
    q = ContextQuery.pattern(ex("x"), scopes=[MINOR, ENDURING, MINOR])
    assert ids(session, q) == [mi, e]


def test_limit_truncates_the_merged_result(session):
    a = session.add(ENDURING, fact("x", "p", "a"))
    b = session.add(MAJOR, fact("x", "p", "b"))
    session.add(MINOR, fact("x", "p", "c"))
    assert ids(session, ContextQuery.pattern(ex("x"), limit=2)) == [a, b]


def test_query_stats_count_scopes_and_matching_quads(session):
    session.add(
        MAJOR,
        ContextItem.from_triples([triple("a", "p", ex("b")), triple("a", "q", ex("c"))], prov()),
    )
    stats = session.query(ContextQuery.pattern(ex("a"))).query_stats
    assert stats.scopes_consulted == 3
    assert stats.quads_matched == 2
    assert stats.auto_promoted == 0


def test_sparql_queries_are_not_yet_implemented(session):
    with pytest.raises(NotYetImplementedError) as e:
        session.query(ContextQuery.sparql("ASK {}"))
    assert e.value.feature == "sparql"


def test_similarity_queries_are_not_yet_implemented(session):
    session.add(MAJOR, ContextItem.chunk("hi", "text/plain", prov(), embedding=[1.0]))
    with pytest.raises(NotYetImplementedError) as e:
        session.query(ContextQuery.similarity([1.0], 1))
    assert e.value.feature == "ann_index"


def test_inference_queries_are_not_yet_implemented(session):
    with pytest.raises(NotYetImplementedError) as e:
        session.query(ContextQuery.inference(scopes=ENDURING))
    assert e.value.feature == "owl_el_reasoner"


def test_a_reasoner_hint_is_not_yet_implemented(session):
    with pytest.raises(NotYetImplementedError) as e:
        session.query(everything_in(MAJOR).with_reasoner_hint(MAJOR))
    assert e.value.feature == "owl_el_reasoner"


def test_refused_queries_are_not_counted_as_served(session):
    with pytest.raises(SessionError):
        session.query(ContextQuery.sparql("ASK {}"))
    assert session.close().queries_served == 0


def test_bad_scope_and_term_arguments_raise_type_error():
    with pytest.raises(TypeError):
        ContextQuery.pattern(scopes="major")
    with pytest.raises(TypeError):
        ContextQuery.pattern(Literal("not a subject"))
    with pytest.raises(TypeError):
        ContextQuery.pattern(predicate=Literal("not a predicate"))
