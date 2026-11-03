//! Shared test support for `semanticore-core`'s integration tests.
//!
//! The storage-facing pieces (quad strategies, the reference `MemStore`,
//! the RFC 0002 contract) live in the dev-only `semanticore-storage-testkit`
//! crate so storage backends can run the identical suite; they are
//! re-exported here. The `any_*` strategies, which only the model tests
//! use, stay local.
//!
//! Not every integration-test binary uses every helper here.
#![allow(dead_code, unused_imports)]

use proptest::prelude::*;
use semanticore_core::model::{BlankNode, Literal, NamedNode, Term};

pub use semanticore_storage_testkit::strategies::*;
pub use semanticore_storage_testkit::{matches_pattern, MemStore};

// `any_*`: arbitrary Unicode strings, for properties that must hold for
// every input (accessor round trips, Eq/Hash consistency).

pub fn any_named() -> impl Strategy<Value = NamedNode> {
    any::<String>().prop_map(NamedNode::new)
}

pub fn any_blank() -> impl Strategy<Value = BlankNode> {
    any::<String>().prop_map(BlankNode::new)
}

pub fn any_literal() -> impl Strategy<Value = Literal> {
    prop_oneof![
        any::<String>().prop_map(Literal::new_simple),
        (any::<String>(), any_named()).prop_map(|(v, d)| Literal::new_typed(v, d)),
        (any::<String>(), any::<String>()).prop_map(|(v, l)| Literal::new_lang(v, l)),
    ]
}

pub fn any_term() -> impl Strategy<Value = Term> {
    prop_oneof![
        any_named().prop_map(Term::Named),
        any_blank().prop_map(Term::Blank),
        any_literal().prop_map(Term::Literal),
    ]
}
