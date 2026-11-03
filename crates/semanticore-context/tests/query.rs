//! `ContextQuery`, `ContextQueryResult` and `Session::query`.

mod common;

use common::*;
use semanticore_context::{
    ContextItem, ContextQuery, ContextQueryKind, ContextScope, InferenceQuery, QuadPattern,
    ScopeSelection, SessionError, Unimplemented,
};
use semanticore_core::model::Literal;

#[test]
fn context_query_builders_fill_the_rfc_fields() {
    let q = ContextQuery::pattern(ScopeSelection::All, QuadPattern::default());
    assert_eq!(q.scopes, ScopeSelection::All);
    assert_eq!(q.kind, ContextQueryKind::Pattern(QuadPattern::default()));
    assert_eq!(q.limit, None);
    assert_eq!(q.reasoner_hint, None);

    let q = ContextQuery::new(
        ScopeSelection::Single(ContextScope::Major),
        ContextQueryKind::Sparql("ASK {}".into()),
    )
    .with_limit(3)
    .with_reasoner_hint(ContextScope::Major);
    assert_eq!(q.limit, Some(3));
    assert_eq!(q.reasoner_hint, Some(ContextScope::Major));

    let p = QuadPattern::default()
        .with_subject(ex("s"))
        .with_predicate(ex("p"))
        .with_object(ex("o"));
    assert_eq!(p.subject, Some(ex("s").into()));
    assert_eq!(p.predicate, Some(ex("p")));
    assert_eq!(p.object, Some(ex("o").into()));
}

#[test]
fn pattern_query_returns_matching_items_with_score_one() {
    let (_dir, mut s) = session();
    let alice = s
        .add(ContextScope::Major, fact("alice", "knows", "bob"))
        .unwrap();
    s.add(ContextScope::Major, fact("carol", "knows", "bob"))
        .unwrap();
    let res = s.query(subject_in(ContextScope::Major, "alice")).unwrap();
    assert_eq!(res.items.len(), 1);
    let hit = &res.items[0];
    assert_eq!(hit.id, alice);
    assert_eq!(hit.score, 1.0);
    assert_eq!(hit.source_scope, ContextScope::Major);
    assert_eq!(hit.item, fact("alice", "knows", "bob"));
}

#[test]
fn pattern_matches_on_predicate_and_object() {
    let (_dir, mut s) = session();
    let a = s
        .add(ContextScope::Minor, fact("a", "likes", "tea"))
        .unwrap();
    let lit = ContextItem::triples(vec![triple("b", "name", Literal::new_simple("B"))], prov());
    let b = s.add(ContextScope::Minor, lit).unwrap();
    let by_pred = ContextQuery::pattern(
        ScopeSelection::Single(ContextScope::Minor),
        QuadPattern::default().with_predicate(ex("likes")),
    );
    assert_eq!(ids(&s, by_pred), vec![a]);
    let by_obj = ContextQuery::pattern(
        ScopeSelection::Single(ContextScope::Minor),
        QuadPattern::default().with_object(Literal::new_simple("B")),
    );
    assert_eq!(ids(&s, by_obj), vec![b]);
}

#[test]
fn an_item_matching_several_quads_is_returned_once() {
    let (_dir, mut s) = session();
    let item = ContextItem::triples(
        vec![triple("a", "p", ex("b")), triple("a", "q", ex("c"))],
        prov(),
    );
    let id = s.add(ContextScope::Major, item).unwrap();
    assert_eq!(ids(&s, subject_in(ContextScope::Major, "a")), vec![id]);
}

#[test]
fn events_and_chunks_never_match_patterns() {
    let (_dir, mut s) = session();
    s.add(
        ContextScope::Major,
        ContextItem::event("turn", serde_json::json!({}), prov()),
    )
    .unwrap();
    s.add(
        ContextScope::Major,
        ContextItem::chunk("hi", "text/plain", prov()),
    )
    .unwrap();
    assert!(ids(&s, everything_in(ContextScope::Major)).is_empty());
}

#[test]
fn scope_selection_controls_which_tiers_are_consulted() {
    let (_dir, mut s) = session();
    let e = s.add(ContextScope::Enduring, fact("x", "p", "e")).unwrap();
    let ma = s.add(ContextScope::Major, fact("x", "p", "m")).unwrap();
    let mi = s.add(ContextScope::Minor, fact("x", "p", "n")).unwrap();
    let pat = QuadPattern::default().with_subject(ex("x"));

    let single = ContextQuery::pattern(ScopeSelection::Single(ContextScope::Major), pat.clone());
    assert_eq!(ids(&s, single), vec![ma]);

    // `All` merges in durability order: enduring, major, minor.
    let all = ContextQuery::pattern(ScopeSelection::All, pat.clone());
    let res = s.query(all).unwrap();
    let got: Vec<_> = res.items.iter().map(|r| (r.id, r.source_scope)).collect();
    assert_eq!(
        got,
        vec![
            (e, ContextScope::Enduring),
            (ma, ContextScope::Major),
            (mi, ContextScope::Minor)
        ]
    );

    // `Set` consults the named scopes, in the order given, once each.
    let set = ContextQuery::pattern(
        ScopeSelection::Set(vec![
            ContextScope::Minor,
            ContextScope::Enduring,
            ContextScope::Minor,
        ]),
        pat,
    );
    assert_eq!(ids(&s, set), vec![mi, e]);
}

#[test]
fn limit_truncates_the_merged_result() {
    let (_dir, mut s) = session();
    let a = s.add(ContextScope::Enduring, fact("x", "p", "a")).unwrap();
    let b = s.add(ContextScope::Major, fact("x", "p", "b")).unwrap();
    s.add(ContextScope::Minor, fact("x", "p", "c")).unwrap();
    let q = ContextQuery::pattern(
        ScopeSelection::All,
        QuadPattern::default().with_subject(ex("x")),
    )
    .with_limit(2);
    assert_eq!(ids(&s, q), vec![a, b]);
}

#[test]
fn query_stats_count_scopes_and_matching_quads() {
    let (_dir, mut s) = session();
    s.add(
        ContextScope::Major,
        ContextItem::triples(
            vec![triple("a", "p", ex("b")), triple("a", "q", ex("c"))],
            prov(),
        ),
    )
    .unwrap();
    let res = s
        .query(ContextQuery::pattern(
            ScopeSelection::All,
            QuadPattern::default().with_subject(ex("a")),
        ))
        .unwrap();
    assert_eq!(res.query_stats.scopes_consulted, 3);
    assert_eq!(res.query_stats.quads_matched, 2);
    assert_eq!(res.query_stats.auto_promoted, 0);
}

#[test]
fn sparql_queries_are_not_yet_implemented() {
    let (_dir, s) = session();
    let q = ContextQuery::new(
        ScopeSelection::All,
        ContextQueryKind::Sparql("ASK {}".into()),
    );
    assert!(matches!(
        s.query(q),
        Err(SessionError::NotYetImplemented(Unimplemented::Sparql))
    ));
}

#[test]
fn similarity_queries_are_not_yet_implemented() {
    let (_dir, mut s) = session();
    s.add(
        ContextScope::Major,
        ContextItem::chunk("hi", "text/plain", prov()).with_embedding(vec![1.0]),
    )
    .unwrap();
    let q = ContextQuery::new(
        ScopeSelection::All,
        ContextQueryKind::Similarity {
            query_embedding: vec![1.0],
            top_k: 1,
        },
    );
    assert!(matches!(
        s.query(q),
        Err(SessionError::NotYetImplemented(Unimplemented::AnnIndex))
    ));
}

#[test]
fn inference_queries_are_not_yet_implemented() {
    let (_dir, s) = session();
    let q = ContextQuery::new(
        ScopeSelection::Single(ContextScope::Enduring),
        ContextQueryKind::Inference(InferenceQuery {
            pattern: QuadPattern::default(),
        }),
    );
    assert!(matches!(
        s.query(q),
        Err(SessionError::NotYetImplemented(
            Unimplemented::OwlElReasoner
        ))
    ));
}

#[test]
fn a_reasoner_hint_is_not_yet_implemented() {
    let (_dir, s) = session();
    let q = everything_in(ContextScope::Major).with_reasoner_hint(ContextScope::Major);
    assert!(matches!(
        s.query(q),
        Err(SessionError::NotYetImplemented(
            Unimplemented::OwlElReasoner
        ))
    ));
}

#[test]
fn refused_queries_are_not_counted_as_served() {
    let (_dir, s) = session();
    let q = ContextQuery::new(
        ScopeSelection::All,
        ContextQueryKind::Sparql("ASK {}".into()),
    );
    assert!(s.query(q).is_err());
    assert_eq!(s.close().unwrap().queries_served, 0);
}
