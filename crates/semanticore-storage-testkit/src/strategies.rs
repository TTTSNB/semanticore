//! Proptest strategies for quads, tuned for storage properties.
//!
//! The alphabets are tiny on purpose, so generated values collide often.
//! That is what makes idempotency, duplicate and equality properties bite.
//! No string produced here contains `<`, `>` or `"`, and every IRI, blank
//! node label and language tag is syntactically valid RDF, so any
//! conforming backend can store every generated quad.

use proptest::prelude::*;
use semanticore_core::model::{
    xsd, BlankNode, Literal, NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term,
};

pub fn small_iri() -> impl Strategy<Value = String> {
    "[a-c]{1,2}".prop_map(|s| format!("https://example.org/{s}"))
}

pub fn small_named() -> impl Strategy<Value = NamedNode> {
    small_iri().prop_map(NamedNode::new)
}

pub fn small_blank() -> impl Strategy<Value = BlankNode> {
    "[a-c]{1,2}".prop_map(BlankNode::new)
}

/// Datatypes for typed literals. Deliberately excludes `rdf:langString`,
/// which needs a language tag.
pub fn small_datatype() -> impl Strategy<Value = NamedNode> {
    prop_oneof![
        Just(NamedNode::new(xsd::STRING)),
        Just(NamedNode::new("http://www.w3.org/2001/XMLSchema#integer")),
        small_named(),
    ]
}

// The only two calls to the tagged/typed `Literal` constructors in this
// crate. If those constructors become fallible, only these two bodies change.

/// A typed literal from inputs known to be well-formed.
pub fn typed_literal(value: String, datatype: NamedNode) -> Literal {
    Literal::new_typed(value, datatype)
}

/// A language-tagged literal from inputs known to be well-formed.
pub fn lang_literal(value: String, language: String) -> Literal {
    Literal::new_lang(value, language)
}

/// Well-formed literals: simple, typed (non-langString), or with a non-empty
/// language tag. Includes a mixed-case tag (`en-GB`) so backends that
/// case-fold tags are caught.
pub fn small_literal() -> impl Strategy<Value = Literal> {
    prop_oneof![
        "[a-c ]{0,3}".prop_map(Literal::new_simple),
        ("[a-c ]{0,3}", small_datatype()).prop_map(|(v, d)| typed_literal(v, d)),
        ("[a-c ]{0,3}", "(en|fr|en-GB)").prop_map(|(v, l)| lang_literal(v, l)),
    ]
}

pub fn small_subject() -> impl Strategy<Value = NamedOrBlankNode> {
    prop_oneof![
        small_named().prop_map(NamedOrBlankNode::Named),
        small_blank().prop_map(NamedOrBlankNode::Blank),
    ]
}

pub fn small_term() -> impl Strategy<Value = Term> {
    prop_oneof![
        small_named().prop_map(Term::Named),
        small_blank().prop_map(Term::Blank),
        small_literal().prop_map(Term::Literal),
    ]
}

pub fn small_graph() -> impl Strategy<Value = NamedOrDefault> {
    prop_oneof![
        Just(NamedOrDefault::Default),
        small_named().prop_map(NamedOrDefault::Named),
    ]
}

pub fn small_quad() -> impl Strategy<Value = Quad> {
    (small_subject(), small_named(), small_term(), small_graph())
        .prop_map(|(s, p, o, g)| Quad::new(s, p, o, g))
}

pub fn small_quads(max: usize) -> impl Strategy<Value = Vec<Quad>> {
    prop::collection::vec(small_quad(), 0..max)
}

/// A pattern position: `None` (wildcard) or `Some` drawn from `inner`.
pub fn maybe<T: std::fmt::Debug + Clone>(
    inner: impl Strategy<Value = T>,
) -> impl Strategy<Value = Option<T>> {
    prop_oneof![Just(None), inner.prop_map(Some)]
}
