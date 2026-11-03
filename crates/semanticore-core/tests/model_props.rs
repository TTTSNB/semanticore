//! Property tests for the RDF term model (`semanticore_core::model`) and the
//! legacy `Iri` type: accessor round trips, constructor invariants, `From`
//! conversions, `Eq`/`Hash`/`Ord` consistency, and `Display` shape.
//!
//! The crate exposes no parser or serializer; `Display` is the only textual
//! form, so it is checked for shape and (on a safe alphabet) injectivity.

mod common;

use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use common::*;
use proptest::prelude::*;
use semanticore_core::model::{
    xsd, BlankNode, Literal, NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term,
    RDF_LANG_STRING,
};
use semanticore_core::Iri;

fn hash_of<T: Hash>(t: &T) -> u64 {
    let mut h = DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

/// `a == b` implies `hash(a) == hash(b)`; also reflexivity via clone.
fn check_eq_hash<T: Eq + Hash + Clone + std::fmt::Debug>(a: &T, b: &T) {
    let c = a.clone();
    assert_eq!(a, &c);
    assert_eq!(hash_of(a), hash_of(&c));
    if a == b {
        assert_eq!(hash_of(a), hash_of(b), "equal values hashed differently");
    }
    let (ab, ba) = (a == b, b == a);
    assert_eq!(ab, ba, "equality not symmetric");
}

/// A `HashSet` deduplicates exactly as `==` does. Catches any Hash/Eq
/// disagreement across a whole batch, not just pairs.
fn check_hashset_dedup<T: Eq + Hash + Clone>(items: &[T]) {
    let mut by_eq: Vec<T> = Vec::new();
    for it in items {
        if !by_eq.iter().any(|x| x == it) {
            by_eq.push(it.clone());
        }
    }
    let set: HashSet<T> = items.iter().cloned().collect();
    assert_eq!(set.len(), by_eq.len());
    for it in items {
        assert!(set.contains(it));
    }
}

proptest! {
    // ---- accessor round trips ---------------------------------------------

    #[test]
    fn named_node_round_trips(s in any::<String>()) {
        let x = NamedNode::new(s.clone());
        prop_assert_eq!(x.as_str(), s.as_str());
    }

    #[test]
    fn blank_node_round_trips(s in any::<String>()) {
        let x = BlankNode::new(s.clone());
        prop_assert_eq!(x.as_str(), s.as_str());
    }

    #[test]
    fn iri_round_trips(s in any::<String>()) {
        let x = Iri::new(s.clone());
        prop_assert_eq!(x.as_str(), s.as_str());
    }

    #[test]
    fn literal_constructors_round_trip(v in any::<String>(), l in any::<String>(), d in any_named()) {
        let simple = Literal::new_simple(v.clone());
        prop_assert_eq!(simple.value(), v.as_str());
        prop_assert_eq!(simple.datatype().as_str(), xsd::STRING);
        prop_assert_eq!(simple.language(), None);

        let typed = Literal::new_typed(v.clone(), d.clone());
        prop_assert_eq!(typed.value(), v.as_str());
        prop_assert_eq!(typed.datatype(), &d);
        prop_assert_eq!(typed.language(), None);

        let lang = Literal::new_lang(v.clone(), l.clone());
        prop_assert_eq!(lang.value(), v.as_str());
        prop_assert_eq!(lang.datatype().as_str(), RDF_LANG_STRING);
        prop_assert_eq!(lang.language(), Some(l.as_str()));
    }

    /// RDF 1.1 well-formedness — a literal carries a language tag iff its
    /// datatype is `rdf:langString`, and the tag is non-empty — holds for
    /// every literal the generators produce. The two inputs outside this
    /// domain (typed `rdf:langString`, empty tag) are pinned in
    /// `tests/regressions.rs`.
    #[test]
    fn literal_well_formed(l in small_literal()) {
        let is_lang_dt = l.datatype().as_str() == RDF_LANG_STRING;
        prop_assert_eq!(is_lang_dt, l.language().is_some());
        if let Some(tag) = l.language() {
            prop_assert!(!tag.is_empty());
        }
    }

    // ---- equality semantics ----------------------------------------------

    /// A simple literal is the same term as an explicitly `xsd:string`-typed
    /// one (RDF 1.1 §3.3), and hashes the same.
    #[test]
    fn simple_literal_equals_xsd_string_typed(v in any::<String>()) {
        let a = Literal::new_simple(v.clone());
        let b = Literal::new_typed(v, NamedNode::new(xsd::STRING));
        prop_assert_eq!(&a, &b);
        prop_assert_eq!(hash_of(&a), hash_of(&b));
    }

    /// Literal equality is component-wise (lexical form, datatype, tag).
    #[test]
    fn literal_equality_is_componentwise(
        v1 in "[ab]{0,2}", v2 in "[ab]{0,2}",
        d1 in small_datatype(), d2 in small_datatype(),
        l1 in "(en|fr)", l2 in "(en|fr)",
    ) {
        prop_assert_eq!(
            Literal::new_typed(v1.clone(), d1.clone()) == Literal::new_typed(v2.clone(), d2.clone()),
            v1 == v2 && d1 == d2
        );
        prop_assert_eq!(
            Literal::new_lang(v1.clone(), l1.clone()) == Literal::new_lang(v2.clone(), l2.clone()),
            v1 == v2 && l1 == l2
        );
        prop_assert_ne!(Literal::new_lang(v1.clone(), l1), Literal::new_simple(v1));
    }

    /// Term kinds never compare equal across variants, even with the same
    /// underlying string.
    #[test]
    fn term_variants_are_disjoint(s in any::<String>()) {
        let n = Term::Named(NamedNode::new(s.clone()));
        let b = Term::Blank(BlankNode::new(s.clone()));
        let l = Term::Literal(Literal::new_simple(s.clone()));
        prop_assert_ne!(&n, &b);
        prop_assert_ne!(&n, &l);
        prop_assert_ne!(&b, &l);
        prop_assert_ne!(
            NamedOrBlankNode::Named(NamedNode::new(s.clone())),
            NamedOrBlankNode::Blank(BlankNode::new(s.clone()))
        );
        prop_assert_ne!(NamedOrDefault::Named(NamedNode::new(s)), NamedOrDefault::Default);
    }

    // ---- Eq / Hash consistency -------------------------------------------

    #[test]
    fn eq_hash_consistent_terms(a in small_term(), b in small_term()) {
        check_eq_hash(&a, &b);
    }

    #[test]
    fn eq_hash_consistent_any_terms(a in any_term(), b in any_term()) {
        check_eq_hash(&a, &b);
    }

    #[test]
    fn eq_hash_consistent_quads(a in small_quad(), b in small_quad()) {
        check_eq_hash(&a, &b);
        check_eq_hash(&a.subject, &b.subject);
        check_eq_hash(&a.graph, &b.graph);
    }

    #[test]
    fn eq_hash_consistent_iri(a in "[ab]{0,2}", b in "[ab]{0,2}") {
        check_eq_hash(&Iri::new(a.clone()), &Iri::new(b.clone()));
        prop_assert_eq!(Iri::new(a.clone()) == Iri::new(b.clone()), a == b);
    }

    #[test]
    fn hashset_dedups_like_eq_terms(items in prop::collection::vec(small_term(), 0..32)) {
        check_hashset_dedup(&items);
    }

    #[test]
    fn hashset_dedups_like_eq_quads(items in small_quads(32)) {
        check_hashset_dedup(&items);
    }

    // ---- NamedNode ordering ----------------------------------------------

    /// `Ord` agrees with `Eq` and with the IRI string order, and is a total
    /// order (antisymmetric, transitive).
    #[test]
    fn named_node_ord_is_total_and_consistent(
        a in any::<String>(), b in any::<String>(), c in any::<String>()
    ) {
        let (na, nb, nc) = (NamedNode::new(a.clone()), NamedNode::new(b.clone()), NamedNode::new(c));
        prop_assert_eq!(na.cmp(&nb), a.cmp(&b));
        prop_assert_eq!(na.cmp(&nb) == Ordering::Equal, na == nb);
        prop_assert_eq!(na.partial_cmp(&nb), Some(na.cmp(&nb)));
        prop_assert_eq!(na.cmp(&nb), nb.cmp(&na).reverse());
        if na <= nb && nb <= nc {
            prop_assert!(na <= nc);
        }
    }

    // ---- From conversions -------------------------------------------------

    #[test]
    fn from_conversions_commute(n in any_named(), b in any_blank(), l in any_literal()) {
        prop_assert_eq!(NamedOrBlankNode::from(n.clone()), NamedOrBlankNode::Named(n.clone()));
        prop_assert_eq!(NamedOrBlankNode::from(b.clone()), NamedOrBlankNode::Blank(b.clone()));
        prop_assert_eq!(Term::from(n.clone()), Term::Named(n.clone()));
        prop_assert_eq!(Term::from(b.clone()), Term::Blank(b.clone()));
        prop_assert_eq!(Term::from(l.clone()), Term::Literal(l));
        // Going through NamedOrBlankNode gives the same Term as going direct.
        prop_assert_eq!(Term::from(NamedOrBlankNode::from(n.clone())), Term::from(n));
        prop_assert_eq!(Term::from(NamedOrBlankNode::from(b.clone())), Term::from(b));
    }

    // ---- Quad --------------------------------------------------------------

    #[test]
    fn quad_constructors_store_fields(
        s in small_subject(), p in small_named(), o in small_term(), g in small_graph()
    ) {
        let q = Quad::new(s.clone(), p.clone(), o.clone(), g.clone());
        prop_assert_eq!(&q.subject, &s);
        prop_assert_eq!(&q.predicate, &p);
        prop_assert_eq!(&q.object, &o);
        prop_assert_eq!(&q.graph, &g);
        prop_assert_eq!(
            Quad::triple(s.clone(), p.clone(), o.clone()),
            Quad::new(s, p, o, NamedOrDefault::Default)
        );
    }

    // ---- Display -----------------------------------------------------------

    #[test]
    fn display_shapes(n in any_named(), b in any_blank(), v in any::<String>(), l in any::<String>()) {
        prop_assert_eq!(n.to_string(), format!("<{}>", n.as_str()));
        prop_assert_eq!(b.to_string(), format!("_:{}", b.as_str()));
        prop_assert_eq!(
            Literal::new_typed(v.clone(), n.clone()).to_string(),
            format!("\"{v}\"^^<{}>", n.as_str())
        );
        prop_assert_eq!(
            Literal::new_simple(v.clone()).to_string(),
            format!("\"{v}\"^^<{}>", xsd::STRING)
        );
        prop_assert_eq!(Literal::new_lang(v.clone(), l.clone()).to_string(), format!("\"{v}\"@{l}"));
    }

    /// `Term` / `NamedOrBlankNode` display delegates to the wrapped node.
    #[test]
    fn display_delegates(t in any_term(), s in small_subject()) {
        let inner = match &t {
            Term::Named(n) => n.to_string(),
            Term::Blank(b) => b.to_string(),
            Term::Literal(l) => l.to_string(),
        };
        prop_assert_eq!(t.to_string(), inner);
        let inner = match &s {
            NamedOrBlankNode::Named(n) => n.to_string(),
            NamedOrBlankNode::Blank(b) => b.to_string(),
        };
        prop_assert_eq!(s.to_string(), inner);
        prop_assert_eq!(Term::from(s.clone()).to_string(), s.to_string());
    }

    /// When no IRI, label or lexical form contains `<`, `>` or `"`, distinct
    /// terms display distinctly.
    /// (Not true in general: `Display` does not escape. See the PR notes.)
    #[test]
    fn display_is_injective_on_safe_alphabet(a in small_term(), b in small_term()) {
        prop_assert_eq!(a == b, a.to_string() == b.to_string());
    }
}

#[test]
fn well_known_iris_are_exact() {
    assert_eq!(xsd::STRING, "http://www.w3.org/2001/XMLSchema#string");
    assert_eq!(
        RDF_LANG_STRING,
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString"
    );
}

#[test]
fn named_or_default_named_holds_graph() {
    let g = NamedNode::new("https://example.org/g");
    let slot = NamedOrDefault::Named(g.clone());
    assert!(matches!(&slot, NamedOrDefault::Named(x) if x == &g));
    assert_ne!(slot, NamedOrDefault::Default);
}
