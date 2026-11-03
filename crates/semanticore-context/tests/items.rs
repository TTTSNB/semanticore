//! `ContextItem`, `Session::add`, `Session::add_batch`, `Session::remove`.

mod common;

use common::*;
use semanticore_context::{ContextItem, ContextPayload, ContextScope, Provenance, SessionError};
use semanticore_core::model::{NamedOrDefault, Quad};

#[test]
fn context_item_constructors_fill_the_rfc_fields() {
    let t = ContextItem::triples(vec![triple("a", "p", ex("b"))], prov());
    assert_eq!(
        t.payload,
        ContextPayload::Triples(vec![triple("a", "p", ex("b"))])
    );
    assert_eq!(t.provenance, prov());
    assert_eq!(t.scope_hint, None);
    assert_eq!(t.embedding, None);

    let e = ContextItem::event("turn", serde_json::json!({"role": "user"}), prov());
    assert_eq!(
        e.payload,
        ContextPayload::Event {
            kind: "turn".into(),
            body: serde_json::json!({"role": "user"}),
        }
    );

    let c = ContextItem::chunk("text", "text/plain", prov())
        .with_scope_hint(ContextScope::Minor)
        .with_embedding(vec![1.0, 0.0]);
    assert_eq!(
        c.payload,
        ContextPayload::Chunk {
            content: "text".into(),
            content_type: "text/plain".into(),
        }
    );
    assert_eq!(c.scope_hint, Some(ContextScope::Minor));
    assert_eq!(c.embedding, Some(vec![1.0, 0.0]));

    let manual = ContextItem::new(ContextPayload::Triples(vec![]), prov());
    assert_eq!(manual, ContextItem::triples(vec![], prov()));
}

#[test]
fn provenance_records_source_and_derivation() {
    let p = Provenance::new("agent");
    assert_eq!(p.source, "agent");
    assert!(p.derived_from.is_empty());
}

#[test]
fn add_records_the_named_scope_and_returns_distinct_ids() {
    let (_dir, mut s) = session();
    let a = s.add(ContextScope::Enduring, fact("a", "p", "b")).unwrap();
    let b = s.add(ContextScope::Major, fact("a", "p", "b")).unwrap();
    let c = s.add(ContextScope::Minor, fact("a", "p", "b")).unwrap();
    assert!(a != b && b != c && a != c);
    assert_eq!(s.scope_of(a), Some(ContextScope::Enduring));
    assert_eq!(s.scope_of(b), Some(ContextScope::Major));
    assert_eq!(s.scope_of(c), Some(ContextScope::Minor));
    for scope in ContextScope::ALL {
        assert_eq!(s.item_count(scope), 1);
    }
}

#[test]
fn the_add_scope_overrides_the_scope_hint() {
    let (_dir, mut s) = session();
    let item = fact("a", "p", "b").with_scope_hint(ContextScope::Enduring);
    let id = s.add(ContextScope::Minor, item.clone()).unwrap();
    assert_eq!(s.scope_of(id), Some(ContextScope::Minor));
    // The hint is kept on the item as given.
    let snap = s.snapshot(ContextScope::Minor).unwrap();
    assert_eq!(snap.items, vec![(id, item)]);
}

#[test]
fn add_refuses_items_without_provenance() {
    let (_dir, mut s) = session();
    let item = ContextItem::triples(vec![triple("a", "p", ex("b"))], Provenance::new(""));
    assert!(matches!(
        s.add(ContextScope::Major, item),
        Err(SessionError::MissingProvenance)
    ));
    assert_eq!(s.item_count(ContextScope::Major), 0);
}

#[test]
fn add_refuses_triples_outside_the_default_graph() {
    let (_dir, mut s) = session();
    let q = Quad::new(
        ex("a").into(),
        ex("p"),
        ex("b").into(),
        NamedOrDefault::Named(ex("g")),
    );
    let item = ContextItem::triples(vec![q], prov());
    assert!(matches!(
        s.add(ContextScope::Major, item),
        Err(SessionError::InvalidItem { .. })
    ));
}

#[test]
fn add_refuses_derivation_from_unknown_items() {
    let (_dir, mut s) = session();
    let a = s.add(ContextScope::Major, fact("a", "p", "b")).unwrap();
    s.remove(ContextScope::Major, &a).unwrap();
    let mut item = fact("c", "p", "d");
    item.provenance = prov().with_derived_from([a]);
    assert!(matches!(
        s.add(ContextScope::Major, item),
        Err(SessionError::UnknownItem(id)) if id == a
    ));
}

#[test]
fn add_batch_returns_ids_in_input_order() {
    let (_dir, mut s) = session();
    let items = [
        fact("a", "p", "b"),
        fact("c", "p", "d"),
        fact("e", "p", "f"),
    ];
    let ids = s.add_batch(ContextScope::Major, &items).unwrap();
    assert_eq!(ids.len(), 3);
    let snap = s.snapshot(ContextScope::Major).unwrap();
    for (id, item) in ids.iter().zip(&items) {
        assert!(snap.items.contains(&(*id, item.clone())));
    }
}

#[test]
fn add_batch_is_all_or_nothing_on_invalid_items() {
    let (_dir, mut s) = session();
    let bad = ContextItem::chunk("x", "text/plain", Provenance::new(""));
    let res = s.add_batch(ContextScope::Major, &[fact("a", "p", "b"), bad]);
    assert!(matches!(res, Err(SessionError::MissingProvenance)));
    assert_eq!(s.item_count(ContextScope::Major), 0);
    assert!(ids(&s, everything_in(ContextScope::Major)).is_empty());
}

#[test]
fn add_batch_accepts_derivation_from_existing_items() {
    let (_dir, mut s) = session();
    let a = s.add(ContextScope::Major, fact("a", "p", "b")).unwrap();
    let mut derived = fact("c", "p", "d");
    derived.provenance = prov().with_derived_from([a]);
    let ids = s.add_batch(ContextScope::Minor, &[derived]).unwrap();
    assert_eq!(s.scope_of(ids[0]), Some(ContextScope::Minor));
}

#[test]
fn remove_reports_whether_the_item_was_in_the_named_scope() {
    let (_dir, mut s) = session();
    let id = s.add(ContextScope::Major, fact("a", "p", "b")).unwrap();
    assert!(!s.remove(ContextScope::Minor, &id).unwrap(), "wrong scope");
    assert_eq!(s.scope_of(id), Some(ContextScope::Major));
    assert!(s.remove(ContextScope::Major, &id).unwrap());
    assert_eq!(s.scope_of(id), None);
    assert!(!s.remove(ContextScope::Major, &id).unwrap(), "already gone");
    assert!(ids(&s, everything_in(ContextScope::Major)).is_empty());
}

#[test]
fn items_sharing_a_triple_are_independent() {
    let (_dir, mut s) = session();
    let a = s.add(ContextScope::Major, fact("a", "p", "b")).unwrap();
    let b = s.add(ContextScope::Major, fact("a", "p", "b")).unwrap();
    assert_eq!(ids(&s, subject_in(ContextScope::Major, "a")), vec![a, b]);
    s.remove(ContextScope::Major, &a).unwrap();
    assert_eq!(ids(&s, subject_in(ContextScope::Major, "a")), vec![b]);
}

#[test]
fn add_refuses_non_finite_embeddings() {
    let (_dir, mut s) = session();
    let item = fact("a", "p", "b").with_embedding(vec![1.0, f32::NAN]);
    assert!(matches!(
        s.add(ContextScope::Major, item),
        Err(SessionError::InvalidItem { .. })
    ));
}
