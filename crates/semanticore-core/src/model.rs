//! Canonical RDF term model.
//!
//! These types are the lingua franca shared by the [`crate::storage`] traits and
//! every reasoner trait in [`crate::reasoner`], [`crate::shacl`], and
//! [`crate::datalog`]. They pin the standard RDF term hierarchy (RFC 0002 §"Trait
//! surface"): subjects are [`NamedOrBlankNode`], predicates are [`NamedNode`],
//! objects are [`Term`], and the graph slot is [`NamedOrDefault`].
//!
//! The model is deliberately backend-neutral — it carries no Oxigraph, Polars, or
//! Arrow types — so that any `Storage` backend and any reasoner can round-trip
//! terms through the same representation without a conversion that depends on a
//! specific store's quirks.

use std::fmt;

/// Well-known datatype IRIs used by [`Literal`] constructors.
pub mod xsd {
    /// `http://www.w3.org/2001/XMLSchema#string` — the datatype of a plain literal.
    pub const STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
}

/// `http://www.w3.org/1999/02/22-rdf-syntax-ns#langString` — datatype of a
/// language-tagged string.
pub const RDF_LANG_STRING: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString";

/// An IRI-identified node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NamedNode {
    iri: String,
}

impl NamedNode {
    /// Construct a named node from an IRI string. The IRI is stored verbatim;
    /// validation/normalization is the responsibility of the producing layer.
    pub fn new(iri: impl Into<String>) -> Self {
        Self { iri: iri.into() }
    }

    /// The IRI as a string slice.
    pub fn as_str(&self) -> &str {
        &self.iri
    }
}

impl fmt::Display for NamedNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}>", self.iri)
    }
}

/// A blank node, identified by a store-local label.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlankNode {
    id: String,
}

impl BlankNode {
    /// Construct a blank node from a local label (without the `_:` prefix).
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }

    /// The blank-node label as a string slice.
    pub fn as_str(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for BlankNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "_:{}", self.id)
    }
}

/// An RDF literal: a lexical form paired with a datatype IRI and, for
/// language-tagged strings, a language tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Literal {
    value: String,
    datatype: NamedNode,
    language: Option<String>,
}

/// Why a [`Literal`] constructor rejected its input.
///
/// RDF 1.1 Concepts §3.3: a literal has a language tag if and only if its
/// datatype IRI is `rdf:langString`, and the tag is non-empty. The fallible
/// constructors return this error instead of building a literal that breaks
/// that rule, so every `Literal` value is well-formed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LiteralError {
    /// [`Literal::new_typed`] was given `rdf:langString` as the datatype. A
    /// language-tagged string needs a tag; build it with [`Literal::new_lang`].
    #[error("datatype rdf:langString requires a language tag; use Literal::new_lang")]
    LangStringWithoutTag,
    /// [`Literal::new_lang`] was given an empty language tag.
    #[error("language tag must be non-empty")]
    EmptyLanguageTag,
}

impl Literal {
    /// A plain `xsd:string` literal. Always well-formed.
    pub fn new_simple(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            datatype: NamedNode::new(xsd::STRING),
            language: None,
        }
    }

    /// A typed literal with an explicit datatype.
    ///
    /// # Errors
    ///
    /// Returns [`LiteralError::LangStringWithoutTag`] if `datatype` is
    /// `rdf:langString` ([`RDF_LANG_STRING`]): such a literal must carry a
    /// language tag, so use [`Literal::new_lang`] instead.
    pub fn new_typed(value: impl Into<String>, datatype: NamedNode) -> Result<Self, LiteralError> {
        if datatype.as_str() == RDF_LANG_STRING {
            return Err(LiteralError::LangStringWithoutTag);
        }
        Ok(Self {
            value: value.into(),
            datatype,
            language: None,
        })
    }

    /// A language-tagged string (`rdf:langString`).
    ///
    /// The tag is stored verbatim; only emptiness is checked. Full BCP 47
    /// validation and case normalization are left to the producing layer, as
    /// for [`NamedNode::new`].
    ///
    /// # Errors
    ///
    /// Returns [`LiteralError::EmptyLanguageTag`] if `language` is empty.
    pub fn new_lang(
        value: impl Into<String>,
        language: impl Into<String>,
    ) -> Result<Self, LiteralError> {
        let language = language.into();
        if language.is_empty() {
            return Err(LiteralError::EmptyLanguageTag);
        }
        Ok(Self {
            value: value.into(),
            datatype: NamedNode::new(RDF_LANG_STRING),
            language: Some(language),
        })
    }

    /// The lexical form.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The datatype IRI.
    pub fn datatype(&self) -> &NamedNode {
        &self.datatype
    }

    /// The language tag, if this is a language-tagged string.
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.language {
            Some(lang) => write!(f, "\"{}\"@{}", self.value, lang),
            None => write!(f, "\"{}\"^^{}", self.value, self.datatype),
        }
    }
}

/// A node that may appear in subject position: an IRI or a blank node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NamedOrBlankNode {
    /// An IRI-named node.
    Named(NamedNode),
    /// A blank node.
    Blank(BlankNode),
}

impl fmt::Display for NamedOrBlankNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NamedOrBlankNode::Named(n) => n.fmt(f),
            NamedOrBlankNode::Blank(b) => b.fmt(f),
        }
    }
}

/// Any RDF term that may appear in object position.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Term {
    /// An IRI-named node.
    Named(NamedNode),
    /// A blank node.
    Blank(BlankNode),
    /// A literal.
    Literal(Literal),
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Term::Named(n) => n.fmt(f),
            Term::Blank(b) => b.fmt(f),
            Term::Literal(l) => l.fmt(f),
        }
    }
}

/// The graph slot of a quad: a named graph or the default graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NamedOrDefault {
    /// A named graph.
    Named(NamedNode),
    /// The default graph.
    Default,
}

/// A quad: subject, predicate, object, graph. Default-graph triples carry
/// [`NamedOrDefault::Default`] in the graph slot (RFC 0002 resolved "Quad vs
/// triple": use `Quad` universally).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Quad {
    /// Subject.
    pub subject: NamedOrBlankNode,
    /// Predicate.
    pub predicate: NamedNode,
    /// Object.
    pub object: Term,
    /// Graph (named, or the default graph).
    pub graph: NamedOrDefault,
}

impl Quad {
    /// Construct a quad in an explicit graph.
    pub fn new(
        subject: NamedOrBlankNode,
        predicate: NamedNode,
        object: Term,
        graph: NamedOrDefault,
    ) -> Self {
        Self {
            subject,
            predicate,
            object,
            graph,
        }
    }

    /// Construct a quad in the default graph.
    pub fn triple(subject: NamedOrBlankNode, predicate: NamedNode, object: Term) -> Self {
        Self::new(subject, predicate, object, NamedOrDefault::Default)
    }
}

impl From<NamedNode> for NamedOrBlankNode {
    fn from(n: NamedNode) -> Self {
        NamedOrBlankNode::Named(n)
    }
}

impl From<BlankNode> for NamedOrBlankNode {
    fn from(b: BlankNode) -> Self {
        NamedOrBlankNode::Blank(b)
    }
}

impl From<NamedNode> for Term {
    fn from(n: NamedNode) -> Self {
        Term::Named(n)
    }
}

impl From<BlankNode> for Term {
    fn from(b: BlankNode) -> Self {
        Term::Blank(b)
    }
}

impl From<Literal> for Term {
    fn from(l: Literal) -> Self {
        Term::Literal(l)
    }
}

impl From<NamedOrBlankNode> for Term {
    fn from(n: NamedOrBlankNode) -> Self {
        match n {
            NamedOrBlankNode::Named(n) => Term::Named(n),
            NamedOrBlankNode::Blank(b) => Term::Blank(b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_node_round_trips_and_displays() {
        let n = NamedNode::new("https://example.org/foo");
        assert_eq!(n.as_str(), "https://example.org/foo");
        assert_eq!(n.to_string(), "<https://example.org/foo>");
    }

    #[test]
    fn blank_node_displays_with_prefix() {
        let b = BlankNode::new("b0");
        assert_eq!(b.as_str(), "b0");
        assert_eq!(b.to_string(), "_:b0");
    }

    #[test]
    fn simple_literal_is_xsd_string() {
        let l = Literal::new_simple("hi");
        assert_eq!(l.value(), "hi");
        assert_eq!(l.datatype().as_str(), xsd::STRING);
        assert_eq!(l.language(), None);
    }

    #[test]
    fn lang_literal_carries_tag_and_langstring_datatype() {
        let l = Literal::new_lang("bonjour", "fr").unwrap();
        assert_eq!(l.language(), Some("fr"));
        assert_eq!(l.datatype().as_str(), RDF_LANG_STRING);
        assert_eq!(l.to_string(), "\"bonjour\"@fr");
    }

    #[test]
    fn typed_literal_keeps_datatype() {
        let dt = NamedNode::new("http://www.w3.org/2001/XMLSchema#integer");
        let l = Literal::new_typed("42", dt.clone()).unwrap();
        assert_eq!(l.datatype(), &dt);
    }

    #[test]
    fn typed_literal_rejects_lang_string_datatype() {
        assert_eq!(
            Literal::new_typed("x", NamedNode::new(RDF_LANG_STRING)),
            Err(LiteralError::LangStringWithoutTag)
        );
    }

    #[test]
    fn lang_literal_rejects_empty_tag() {
        assert_eq!(
            Literal::new_lang("x", ""),
            Err(LiteralError::EmptyLanguageTag)
        );
    }

    #[test]
    fn quad_default_graph_constructor() {
        let q = Quad::triple(
            NamedNode::new("https://example.org/s").into(),
            NamedNode::new("https://example.org/p"),
            Literal::new_simple("o").into(),
        );
        assert_eq!(q.graph, NamedOrDefault::Default);
        assert_eq!(
            q.subject,
            NamedOrBlankNode::Named(NamedNode::new("https://example.org/s"))
        );
    }

    #[test]
    fn term_conversions() {
        let t: Term = BlankNode::new("x").into();
        assert_eq!(t, Term::Blank(BlankNode::new("x")));
        let t2: Term = NamedOrBlankNode::Named(NamedNode::new("https://example.org/a")).into();
        assert_eq!(t2, Term::Named(NamedNode::new("https://example.org/a")));
    }
}
