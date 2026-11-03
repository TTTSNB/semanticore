//! Pinned failing cases found by the property tests. Each is `#[ignore]`d
//! because fixing it would change library behaviour, which is out of scope
//! for the test-only PR that added them. Run them with
//! `cargo test --test regressions -- --ignored`.
//!
//! A fix could make the constructor reject the input (panic or a fallible
//! constructor) or normalise it. These tests accept either a rejection
//! (panic) or a well-formed result. A fallible-constructor fix will need the
//! test rewritten to match the new signature.

use std::panic;

use semanticore_core::model::{Literal, NamedNode, RDF_LANG_STRING};

/// RDF 1.1 Concepts §3.3: a literal is a language-tagged string iff its
/// datatype IRI is `rdf:langString`, and then its language tag is non-empty.
fn assert_well_formed_or_rejected(build: impl FnOnce() -> Literal + panic::UnwindSafe) {
    let Ok(l) = panic::catch_unwind(build) else {
        return; // Rejected at construction: acceptable.
    };
    let is_lang_dt = l.datatype().as_str() == RDF_LANG_STRING;
    assert_eq!(
        is_lang_dt,
        l.language().is_some(),
        "datatype rdf:langString iff a language tag is present; got {l:?}"
    );
    if let Some(tag) = l.language() {
        assert!(!tag.is_empty(), "language tag must be non-empty; got {l:?}");
    }
}

/// DEFECT (spec divergence): `Literal::new_typed` accepts `rdf:langString`
/// as the datatype and returns a literal whose datatype is `rdf:langString`
/// but whose `language()` is `None`. RDF 1.1 does not allow that literal.
/// A backend that enforces the rule would have to reject or rewrite it, so
/// the term would not round-trip through storage unchanged.
///
/// Found by proptest (`literal_well_formed` with `rdf:langString` in the
/// datatype strategy). Minimal input: `value = ""`, `datatype = rdf:langString`.
#[test]
#[ignore = "defect: Literal::new_typed accepts rdf:langString without a language tag"]
fn new_typed_rejects_or_normalises_rdf_lang_string() {
    assert_well_formed_or_rejected(|| Literal::new_typed("", NamedNode::new(RDF_LANG_STRING)));
}

/// DEFECT (spec divergence): `Literal::new_lang` accepts an empty language
/// tag and returns `language() == Some("")`. RDF 1.1 needs a non-empty
/// BCP 47 tag. `Display` then prints `"x"@`, which is not valid
/// N-Triples/Turtle either.
///
/// Found by proptest (`literal_well_formed` over arbitrary tags). Minimal
/// input: `value = ""`, `language = ""`.
#[test]
#[ignore = "defect: Literal::new_lang accepts an empty language tag"]
fn new_lang_rejects_or_normalises_empty_tag() {
    assert_well_formed_or_rejected(|| Literal::new_lang("", ""));
}
