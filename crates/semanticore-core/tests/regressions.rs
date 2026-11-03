//! Regression tests for defects first found by the property tests.
//!
//! Both were pinned as `#[ignore]`d tests when they were found. They now
//! pass: `Literal::new_typed` and `Literal::new_lang` return
//! `Result<Literal, LiteralError>` and reject ill-formed input, so an
//! ill-formed `Literal` cannot be constructed.

use semanticore_core::model::{Literal, LiteralError, NamedNode, RDF_LANG_STRING};

/// RDF 1.1 Concepts §3.3: a literal is a language-tagged string iff its
/// datatype IRI is `rdf:langString`, and then its language tag is non-empty.
fn assert_well_formed(l: &Literal) {
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

/// Formerly a defect: `Literal::new_typed` accepted `rdf:langString` as the
/// datatype and returned a literal whose datatype was `rdf:langString` but
/// whose `language()` was `None`. RDF 1.1 does not allow that literal.
///
/// Found by proptest (`literal_well_formed` with `rdf:langString` in the
/// datatype strategy). Minimal input: `value = ""`, `datatype = rdf:langString`.
#[test]
fn new_typed_rejects_rdf_lang_string() {
    assert_eq!(
        Literal::new_typed("", NamedNode::new(RDF_LANG_STRING)),
        Err(LiteralError::LangStringWithoutTag)
    );
    // The well-formed way to build that literal still works.
    assert_well_formed(&Literal::new_lang("", "en").unwrap());
}

/// Formerly a defect: `Literal::new_lang` accepted an empty language tag and
/// returned `language() == Some("")`, which `Display` printed as `"x"@`.
/// RDF 1.1 needs a non-empty BCP 47 tag.
///
/// Found by proptest (`literal_well_formed` over arbitrary tags). Minimal
/// input: `value = ""`, `language = ""`.
#[test]
fn new_lang_rejects_empty_tag() {
    assert_eq!(
        Literal::new_lang("", ""),
        Err(LiteralError::EmptyLanguageTag)
    );
    assert_well_formed(&Literal::new_lang("", "fr").unwrap());
}

/// The errors render a message that names the rule that was broken.
#[test]
fn literal_error_display() {
    assert!(LiteralError::LangStringWithoutTag
        .to_string()
        .contains("rdf:langString"));
    assert!(LiteralError::EmptyLanguageTag
        .to_string()
        .contains("non-empty"));
}
