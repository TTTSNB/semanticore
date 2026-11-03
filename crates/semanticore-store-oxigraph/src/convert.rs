//! Term conversion between `semanticore_core::model` and `oxigraph::model`,
//! and the mapping of Oxigraph's errors into [`StorageError`].
//!
//! The core model stores terms verbatim and compares them lexically. Oxigraph
//! validates IRIs, blank-node labels and language tags, and its storage layer
//! re-encodes well-formed XSD literals into native values that read back in
//! canonical form (`"01"^^xsd:integer` comes back as `"1"`, `xsd:int` comes
//! back as `xsd:integer`). Storing such a term would silently change it, so
//! the core-to-Oxigraph direction refuses every term that would not read
//! back identical, reporting it as [`Unrepresentable`].

use std::fmt;
use std::str::FromStr;

use oxigraph::model as ox;
use oxsdatatypes::{
    Boolean, Date, DateTime, DayTimeDuration, Decimal, Double, Duration, Float, GDay, GMonth,
    GMonthDay, GYear, GYearMonth, Integer, Time, YearMonthDuration,
};
use semanticore_core::model::{
    xsd, BlankNode, Literal, NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term,
    RDF_LANG_STRING,
};
use semanticore_core::storage::StorageError;

const XSD: &str = "http://www.w3.org/2001/XMLSchema#";

/// Why a term cannot pass between the two models unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Unrepresentable {
    /// Not an absolute IRI per RFC 3987.
    Iri(String),
    /// Not a valid blank-node label.
    BlankNode(String),
    /// Not a valid BCP 47 language tag.
    LanguageTag(String),
    /// `rdf:langString` datatype without a language tag.
    LangStringWithoutTag,
    /// A literal Oxigraph would store in a different lexical form or datatype.
    NonCanonicalLiteral { value: String, datatype: String },
    /// A graph named by a blank node (the core model has named graphs only).
    BlankGraphName(String),
    /// An RDF 1.2 triple term or directional language string.
    Rdf12Term(String),
}

impl fmt::Display for Unrepresentable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Iri(iri) => write!(f, "invalid IRI <{iri}>"),
            Self::BlankNode(id) => write!(f, "invalid blank node label _:{id}"),
            Self::LanguageTag(tag) => write!(f, "invalid language tag @{tag}"),
            Self::LangStringWithoutTag => {
                f.write_str("rdf:langString literal without a language tag")
            }
            Self::NonCanonicalLiteral { value, datatype } => write!(
                f,
                "literal \"{value}\"^^<{datatype}> would be stored in canonical form; \
                 insert the canonical lexical form with the primitive datatype instead"
            ),
            Self::BlankGraphName(id) => {
                write!(f, "graph named by blank node _:{id} has no core equivalent")
            }
            Self::Rdf12Term(term) => write!(f, "RDF 1.2 term {term} has no core equivalent"),
        }
    }
}

impl From<Unrepresentable> for StorageError {
    fn from(u: Unrepresentable) -> Self {
        StorageError::Serialization(u.to_string())
    }
}

/// Map an Oxigraph storage error into the core error type. I/O errors keep
/// their `io::Error` (so `ErrorKind` survives); everything else is wrapped
/// in [`StorageError::Backend`] with the original error as its source.
pub(crate) fn map_err(e: oxigraph::store::StorageError) -> StorageError {
    match e {
        oxigraph::store::StorageError::Io(io) => StorageError::Io(io),
        other => StorageError::Backend(Box::new(other)),
    }
}

// ---------------------------------------------------------------------------
// core -> Oxigraph
// ---------------------------------------------------------------------------

pub(crate) fn named_to_ox(n: &NamedNode) -> Result<ox::NamedNode, Unrepresentable> {
    ox::NamedNode::new(n.as_str()).map_err(|_| Unrepresentable::Iri(n.as_str().to_owned()))
}

fn blank_to_ox(b: &BlankNode) -> Result<ox::BlankNode, Unrepresentable> {
    ox::BlankNode::new(b.as_str()).map_err(|_| Unrepresentable::BlankNode(b.as_str().to_owned()))
}

pub(crate) fn subject_to_ox(s: &NamedOrBlankNode) -> Result<ox::NamedOrBlankNode, Unrepresentable> {
    Ok(match s {
        NamedOrBlankNode::Named(n) => named_to_ox(n)?.into(),
        NamedOrBlankNode::Blank(b) => blank_to_ox(b)?.into(),
    })
}

pub(crate) fn term_to_ox(t: &Term) -> Result<ox::Term, Unrepresentable> {
    Ok(match t {
        Term::Named(n) => named_to_ox(n)?.into(),
        Term::Blank(b) => blank_to_ox(b)?.into(),
        Term::Literal(l) => literal_to_ox(l)?.into(),
    })
}

pub(crate) fn graph_to_ox(g: &NamedOrDefault) -> Result<ox::GraphName, Unrepresentable> {
    Ok(match g {
        NamedOrDefault::Named(n) => named_to_ox(n)?.into(),
        NamedOrDefault::Default => ox::GraphName::DefaultGraph,
    })
}

pub(crate) fn quad_to_ox(q: &Quad) -> Result<ox::Quad, Unrepresentable> {
    Ok(ox::Quad::new(
        subject_to_ox(&q.subject)?,
        named_to_ox(&q.predicate)?,
        term_to_ox(&q.object)?,
        graph_to_ox(&q.graph)?,
    ))
}

fn literal_to_ox(l: &Literal) -> Result<ox::Literal, Unrepresentable> {
    let value = l.value();
    if let Some(tag) = l.language() {
        // Validate the tag, but store it verbatim: Oxigraph's checked
        // constructor lower-cases it, and the core model compares tags
        // case-sensitively.
        ox::Literal::new_language_tagged_literal(value, tag)
            .map_err(|_| Unrepresentable::LanguageTag(tag.to_owned()))?;
        return Ok(ox::Literal::new_language_tagged_literal_unchecked(
            value, tag,
        ));
    }
    let datatype = l.datatype().as_str();
    if datatype == RDF_LANG_STRING {
        return Err(Unrepresentable::LangStringWithoutTag);
    }
    if !round_trips(value, datatype) {
        return Err(Unrepresentable::NonCanonicalLiteral {
            value: value.to_owned(),
            datatype: datatype.to_owned(),
        });
    }
    Ok(ox::Literal::new_typed_literal(
        value,
        named_to_ox(l.datatype())?,
    ))
}

/// `true` if `"value"^^<datatype>` reads back from Oxigraph's storage with
/// the same lexical form and datatype.
///
/// Mirrors `From<LiteralRef> for EncodedTerm` in `oxigraph` 0.5.13
/// (`storage/numeric_encoder.rs`): for the XSD datatypes listed there, a
/// value that parses is stored as a native value and decoded with the
/// primitive type's canonical `Display`; a value that does not parse is
/// stored verbatim. The property test `literal_round_trip` checks this
/// against the real store.
fn round_trips(value: &str, datatype: &str) -> bool {
    fn canonical<T: FromStr + fmt::Display>(value: &str) -> bool {
        value.parse::<T>().is_ok_and(|v| v.to_string() == value)
    }
    fn unparsable<T: FromStr>(value: &str) -> bool {
        value.parse::<T>().is_err()
    }
    let Some(local) = datatype.strip_prefix(XSD) else {
        return true;
    };
    match local {
        "boolean" => canonical::<Boolean>(value) || unparsable::<Boolean>(value),
        "float" => canonical::<Float>(value) || unparsable::<Float>(value),
        "double" => canonical::<Double>(value) || unparsable::<Double>(value),
        "integer" => canonical::<Integer>(value) || unparsable::<Integer>(value),
        "decimal" => canonical::<Decimal>(value) || unparsable::<Decimal>(value),
        "dateTime" => canonical::<DateTime>(value) || unparsable::<DateTime>(value),
        "time" => canonical::<Time>(value) || unparsable::<Time>(value),
        "date" => canonical::<Date>(value) || unparsable::<Date>(value),
        "gYearMonth" => canonical::<GYearMonth>(value) || unparsable::<GYearMonth>(value),
        "gYear" => canonical::<GYear>(value) || unparsable::<GYear>(value),
        "gMonthDay" => canonical::<GMonthDay>(value) || unparsable::<GMonthDay>(value),
        "gDay" => canonical::<GDay>(value) || unparsable::<GDay>(value),
        "gMonth" => canonical::<GMonth>(value) || unparsable::<GMonth>(value),
        "duration" => canonical::<Duration>(value) || unparsable::<Duration>(value),
        "yearMonthDuration" => {
            canonical::<YearMonthDuration>(value) || unparsable::<YearMonthDuration>(value)
        }
        "dayTimeDuration" => {
            canonical::<DayTimeDuration>(value) || unparsable::<DayTimeDuration>(value)
        }
        // Derived types Oxigraph decodes under a different datatype
        // (`xsd:integer`, `xsd:dateTime`): any parsable value changes.
        "byte" | "short" | "int" | "long" | "unsignedByte" | "unsignedShort" | "unsignedInt"
        | "unsignedLong" | "positiveInteger" | "negativeInteger" | "nonPositiveInteger"
        | "nonNegativeInteger" => unparsable::<Integer>(value),
        "dateTimeStamp" => unparsable::<DateTime>(value),
        // `xsd:string` and every other XSD datatype are stored verbatim.
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Oxigraph -> core
// ---------------------------------------------------------------------------

pub(crate) fn named_from_ox(n: &ox::NamedNode) -> NamedNode {
    NamedNode::new(n.as_str())
}

fn subject_from_ox(s: &ox::NamedOrBlankNode) -> NamedOrBlankNode {
    match s {
        ox::NamedOrBlankNode::NamedNode(n) => named_from_ox(n).into(),
        ox::NamedOrBlankNode::BlankNode(b) => BlankNode::new(b.as_str()).into(),
    }
}

fn term_from_ox(t: &ox::Term) -> Result<Term, StorageError> {
    // `ox::Term` gains a `Triple` variant when Oxigraph's `rdf-12` feature is
    // enabled anywhere in the build, hence the catch-all arm.
    #[allow(unreachable_patterns)]
    Ok(match t {
        ox::Term::NamedNode(n) => named_from_ox(n).into(),
        ox::Term::BlankNode(b) => BlankNode::new(b.as_str()).into(),
        ox::Term::Literal(l) => literal_from_ox(l)?.into(),
        other => return Err(Unrepresentable::Rdf12Term(other.to_string()).into()),
    })
}

/// The single place core literals are built from stored ones. Returns a
/// `Result` so that fallible core constructors only change this body.
fn literal_from_ox(l: &ox::Literal) -> Result<Literal, StorageError> {
    let datatype = l.datatype().as_str();
    match l.language() {
        Some(tag) if datatype == RDF_LANG_STRING => Ok(Literal::new_lang(l.value(), tag)),
        // A language tag with another datatype is an RDF 1.2 directional
        // language string, which the core model cannot carry.
        Some(_) => Err(Unrepresentable::Rdf12Term(l.to_string()).into()),
        None if datatype == xsd::STRING => Ok(Literal::new_simple(l.value())),
        None => Ok(Literal::new_typed(l.value(), NamedNode::new(datatype))),
    }
}

fn graph_from_ox(g: &ox::GraphName) -> Result<NamedOrDefault, StorageError> {
    match g {
        ox::GraphName::NamedNode(n) => Ok(NamedOrDefault::Named(named_from_ox(n))),
        ox::GraphName::DefaultGraph => Ok(NamedOrDefault::Default),
        ox::GraphName::BlankNode(b) => {
            Err(Unrepresentable::BlankGraphName(b.as_str().to_owned()).into())
        }
    }
}

pub(crate) fn quad_from_ox(q: &ox::Quad) -> Result<Quad, StorageError> {
    Ok(Quad::new(
        subject_from_ox(&q.subject),
        named_from_ox(&q.predicate),
        term_from_ox(&q.object)?,
        graph_from_ox(&q.graph_name)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(v: &str) -> Literal {
        Literal::new_typed(v, NamedNode::new(format!("{XSD}integer")))
    }

    #[test]
    fn canonical_numeric_literals_pass() {
        assert!(literal_to_ox(&int("1")).is_ok());
        assert!(literal_to_ox(&int("-42")).is_ok());
        // Not an integer at all: stored verbatim as an ill-typed literal.
        assert!(literal_to_ox(&int("abc")).is_ok());
    }

    #[test]
    fn non_canonical_literals_are_refused() {
        for v in ["01", "+1", "-0"] {
            let err = literal_to_ox(&int(v)).unwrap_err();
            assert!(
                matches!(err, Unrepresentable::NonCanonicalLiteral { .. }),
                "{v}"
            );
        }
        // Oxigraph's parser does not trim whitespace, so this one is stored
        // verbatim as an ill-typed literal and reads back unchanged.
        assert!(literal_to_ox(&int(" 1")).is_ok());
        let b = Literal::new_typed("1", NamedNode::new(format!("{XSD}boolean")));
        assert!(literal_to_ox(&b).is_err());
        let i = Literal::new_typed("5", NamedNode::new(format!("{XSD}int")));
        assert!(literal_to_ox(&i).is_err());
        let i = Literal::new_typed("five", NamedNode::new(format!("{XSD}int")));
        assert!(literal_to_ox(&i).is_ok());
    }

    #[test]
    fn language_tags_are_validated_but_kept_verbatim() {
        let l = literal_to_ox(&Literal::new_lang("colour", "en-GB")).unwrap();
        assert_eq!(l.language(), Some("en-GB"));
        assert!(matches!(
            literal_to_ox(&Literal::new_lang("x", "")),
            Err(Unrepresentable::LanguageTag(_))
        ));
        assert!(matches!(
            literal_to_ox(&Literal::new_lang("x", "not a tag")),
            Err(Unrepresentable::LanguageTag(_))
        ));
    }

    #[test]
    fn lang_string_without_tag_is_refused() {
        let l = Literal::new_typed("x", NamedNode::new(RDF_LANG_STRING));
        assert_eq!(
            literal_to_ox(&l),
            Err(Unrepresentable::LangStringWithoutTag)
        );
    }

    #[test]
    fn invalid_iris_and_blank_labels_are_refused() {
        assert!(matches!(
            named_to_ox(&NamedNode::new("not an iri")),
            Err(Unrepresentable::Iri(_))
        ));
        assert!(matches!(
            blank_to_ox(&BlankNode::new("has space")),
            Err(Unrepresentable::BlankNode(_))
        ));
        let typed = Literal::new_typed("x", NamedNode::new("relative"));
        assert!(matches!(
            literal_to_ox(&typed),
            Err(Unrepresentable::Iri(_))
        ));
    }

    #[test]
    fn blank_graph_names_do_not_convert_back() {
        let q = ox::Quad::new(
            ox::NamedNode::new_unchecked("https://example.org/s"),
            ox::NamedNode::new_unchecked("https://example.org/p"),
            ox::NamedNode::new_unchecked("https://example.org/o"),
            ox::BlankNode::new_unchecked("g"),
        );
        let err = quad_from_ox(&q).unwrap_err();
        assert!(matches!(err, StorageError::Serialization(m) if m.contains("_:g")));
    }

    #[test]
    fn unrepresentable_messages_name_the_term() {
        let cases = [
            (Unrepresentable::Iri("x".into()), "<x>"),
            (Unrepresentable::BlankNode("x y".into()), "_:x y"),
            (Unrepresentable::LanguageTag("".into()), "language tag"),
            (Unrepresentable::LangStringWithoutTag, "rdf:langString"),
            (
                Unrepresentable::NonCanonicalLiteral {
                    value: "01".into(),
                    datatype: "dt".into(),
                },
                "\"01\"^^<dt>",
            ),
            (Unrepresentable::BlankGraphName("g".into()), "_:g"),
            (Unrepresentable::Rdf12Term("t".into()), "RDF 1.2"),
        ];
        for (u, needle) in cases {
            let msg = u.to_string();
            assert!(msg.contains(needle), "{msg:?} lacks {needle:?}");
            assert!(matches!(
                StorageError::from(u),
                StorageError::Serialization(_)
            ));
        }
    }

    #[test]
    fn io_errors_keep_their_kind() {
        let e = oxigraph::store::StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "nope",
        ));
        match map_err(e) {
            StorageError::Io(io) => assert_eq!(io.kind(), std::io::ErrorKind::PermissionDenied),
            other => panic!("expected Io, got {other:?}"),
        }
        let other = oxigraph::store::StorageError::Other("boom".into());
        assert!(matches!(map_err(other), StorageError::Backend(_)));
    }
}
