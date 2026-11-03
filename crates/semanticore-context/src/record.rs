//! How items are laid out in a scope's `Storage`.
//!
//! Each item owns a named graph, `urn:semanticore:context:item:<n>`, holding
//! its triples (for [`ContextPayload::Triples`]; empty otherwise). One
//! metadata quad per item, in the graph `urn:semanticore:context:meta`,
//! carries everything else as a JSON literal:
//!
//! ```text
//! <urn:semanticore:context:item:7> <urn:semanticore:context:record> "{...}" <urn:semanticore:context:meta>
//! ```
//!
//! The layout lets a Session rebuild its index from a persistent store on
//! open, and lets two items hold the same triple independently.

use semanticore_core::model::{Literal, NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term};
use serde::{Deserialize, Serialize};

use crate::error::SessionError;
use crate::item::{ContextItem, ContextPayload, ContextScope, ItemId, Provenance};

const ITEM_PREFIX: &str = "urn:semanticore:context:item:";
const META_GRAPH: &str = "urn:semanticore:context:meta";
const RECORD: &str = "urn:semanticore:context:record";

pub(crate) fn item_graph(id: ItemId) -> NamedNode {
    NamedNode::new(format!("{ITEM_PREFIX}{}", id.0))
}

pub(crate) fn parse_item_graph(graph: &NamedNode) -> Option<ItemId> {
    let n = graph.as_str().strip_prefix(ITEM_PREFIX)?;
    // Reject non-canonical spellings ("+1", "01") so one id has one IRI.
    let id: u64 = n.parse().ok()?;
    (id.to_string() == n).then_some(ItemId(id))
}

pub(crate) fn meta_graph() -> NamedOrDefault {
    NamedOrDefault::Named(NamedNode::new(META_GRAPH))
}

pub(crate) fn record_predicate() -> NamedNode {
    NamedNode::new(RECORD)
}

#[derive(Serialize, Deserialize)]
struct Record {
    payload: RecordPayload,
    source: String,
    derived_from: Vec<u64>,
    scope_hint: Option<String>,
    embedding: Option<Vec<f32>>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RecordPayload {
    Triples,
    Event {
        kind: String,
        body: serde_json::Value,
    },
    Chunk {
        content: String,
        content_type: String,
    },
}

/// Every quad that represents `item` in storage.
pub(crate) fn encode(id: ItemId, item: &ContextItem) -> Vec<Quad> {
    let graph = item_graph(id);
    let (payload, triples): (RecordPayload, &[Quad]) = match &item.payload {
        ContextPayload::Triples(t) => (RecordPayload::Triples, t),
        ContextPayload::Event { kind, body } => (
            RecordPayload::Event {
                kind: kind.clone(),
                body: body.clone(),
            },
            &[],
        ),
        ContextPayload::Chunk {
            content,
            content_type,
        } => (
            RecordPayload::Chunk {
                content: content.clone(),
                content_type: content_type.clone(),
            },
            &[],
        ),
    };
    let record = Record {
        payload,
        source: item.provenance.source.clone(),
        derived_from: item.provenance.derived_from.iter().map(|i| i.0).collect(),
        scope_hint: item.scope_hint.map(|s| s.tag().to_owned()),
        embedding: item.embedding.clone(),
    };
    // Serializing plain data to a String cannot fail; embeddings are checked
    // finite before they get here.
    let json = serde_json::to_string(&record).expect("record serializes");
    let mut quads: Vec<Quad> = triples
        .iter()
        .map(|t| {
            Quad::new(
                t.subject.clone(),
                t.predicate.clone(),
                t.object.clone(),
                NamedOrDefault::Named(graph.clone()),
            )
        })
        .collect();
    quads.push(Quad::new(
        NamedOrBlankNode::Named(graph),
        record_predicate(),
        Term::Literal(Literal::new_simple(json)),
        meta_graph(),
    ));
    quads
}

/// Rebuild an item from its metadata literal and the quads of its graph.
pub(crate) fn decode(
    id: ItemId,
    json: &str,
    graph_quads: Vec<Quad>,
) -> Result<ContextItem, SessionError> {
    let record: Record = serde_json::from_str(json).map_err(|e| SessionError::Corrupt {
        reason: format!("metadata of {id}: {e}"),
    })?;
    let payload = match record.payload {
        RecordPayload::Triples => ContextPayload::Triples(
            graph_quads
                .into_iter()
                .map(|q| Quad::triple(q.subject, q.predicate, q.object))
                .collect(),
        ),
        RecordPayload::Event { kind, body } => ContextPayload::Event { kind, body },
        RecordPayload::Chunk {
            content,
            content_type,
        } => ContextPayload::Chunk {
            content,
            content_type,
        },
    };
    let scope_hint = match record.scope_hint {
        None => None,
        Some(tag) => Some(
            ContextScope::from_tag(&tag).ok_or_else(|| SessionError::Corrupt {
                reason: format!("metadata of {id}: unknown scope {tag:?}"),
            })?,
        ),
    };
    Ok(ContextItem {
        payload,
        provenance: Provenance {
            source: record.source,
            derived_from: record.derived_from.into_iter().map(ItemId).collect(),
        },
        scope_hint,
        embedding: record.embedding,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> NamedNode {
        NamedNode::new(format!("https://example.org/{s}"))
    }

    #[test]
    fn item_graph_round_trips_canonical_ids_only() {
        assert_eq!(parse_item_graph(&item_graph(ItemId(42))), Some(ItemId(42)));
        for bad in ["01", "+1", "x", ""] {
            assert_eq!(
                parse_item_graph(&NamedNode::new(format!("{ITEM_PREFIX}{bad}"))),
                None
            );
        }
        assert_eq!(parse_item_graph(&n("1")), None);
    }

    #[test]
    fn encode_then_decode_is_identity() {
        let item = ContextItem::triples(
            vec![Quad::triple(n("a").into(), n("p"), n("b").into())],
            Provenance::new("src").with_derived_from([ItemId(3)]),
        )
        .with_scope_hint(ContextScope::Enduring)
        .with_embedding(vec![0.1, 2.5]);
        let quads = encode(ItemId(9), &item);
        assert_eq!(quads.len(), 2);
        let (meta, payload): (Vec<_>, Vec<_>) =
            quads.into_iter().partition(|q| q.graph == meta_graph());
        let Term::Literal(lit) = &meta[0].object else {
            panic!("literal expected")
        };
        assert_eq!(decode(ItemId(9), lit.value(), payload).unwrap(), item);
    }

    #[test]
    fn decode_reports_corrupt_metadata() {
        assert!(matches!(
            decode(ItemId(1), "not json", vec![]),
            Err(SessionError::Corrupt { .. })
        ));
        let bad_hint = r#"{"payload":{"type":"triples"},"source":"s","derived_from":[],"scope_hint":"huge","embedding":null}"#;
        assert!(matches!(
            decode(ItemId(1), bad_hint, vec![]),
            Err(SessionError::Corrupt { .. })
        ));
    }
}
