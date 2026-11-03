//! Scopes, item handles and the unit of content.

use std::fmt;

use semanticore_core::model::Quad;

/// The three context tiers (RFC 0003 / RFC 0004).
///
/// Scopes are ordered by durability: `Minor < Major < Enduring`. Promotion
/// moves an item to an equal-or-greater scope; demotion moves it to a
/// strictly lesser one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ContextScope {
    /// Turn-lived, ephemeral context. Discarded with the Session.
    Minor,
    /// Session-lived working set. Discarded with the Session at v0.1.
    Major,
    /// Persistent, slowly-changing context. Survives the Session.
    Enduring,
}

impl ContextScope {
    /// Every scope, most durable first. This is also the merge order of
    /// [`crate::ScopeSelection::All`].
    pub const ALL: [ContextScope; 3] = [
        ContextScope::Enduring,
        ContextScope::Major,
        ContextScope::Minor,
    ];

    /// The next less durable scope, if any: the target of a one-step
    /// demotion.
    pub const fn colder(self) -> Option<ContextScope> {
        match self {
            ContextScope::Enduring => Some(ContextScope::Major),
            ContextScope::Major => Some(ContextScope::Minor),
            ContextScope::Minor => None,
        }
    }

    pub(crate) const fn tag(self) -> &'static str {
        match self {
            ContextScope::Enduring => "enduring",
            ContextScope::Major => "major",
            ContextScope::Minor => "minor",
        }
    }

    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.tag() == tag)
    }
}

impl fmt::Display for ContextScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

/// An opaque handle to an item in a [`crate::Session`].
///
/// Stable for the lifetime of a Session. RFC 0004 makes no promise across
/// Session restarts: use the item's IRIs or a content hash for cross-Session
/// identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub(crate) u64);

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "item:{}", self.0)
    }
}

/// Where an item came from. Required on every item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// Who or what produced the item (an agent, a tool, a document IRI).
    /// Must not be empty: the Session refuses items without provenance.
    pub source: String,
    /// Items this one was derived from. An item cited by more than
    /// [`crate::PromotionConfig::major_to_enduring_references`] other items
    /// is auto-promoted from major to enduring.
    pub derived_from: Vec<ItemId>,
}

impl Provenance {
    /// Provenance with a source and no derivation.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            derived_from: Vec::new(),
        }
    }

    /// Record the items this one was derived from.
    pub fn with_derived_from(mut self, items: impl IntoIterator<Item = ItemId>) -> Self {
        self.derived_from.extend(items);
        self
    }
}

/// The payload of a [`ContextItem`].
#[derive(Debug, Clone, PartialEq)]
pub enum ContextPayload {
    /// RDF triples. At v0.1 every quad must be in the default graph: the
    /// Session stores an item's triples in a graph of its own. The payload is
    /// a set; order and duplicates are not preserved across a reopen.
    Triples(Vec<Quad>),
    /// A typed structured event with a JSON body (agent turns, tool calls,
    /// system signals).
    Event {
        /// The event type.
        kind: String,
        /// The event body.
        body: serde_json::Value,
    },
    /// A chunk of text or other content.
    Chunk {
        /// The content.
        content: String,
        /// Its media type, e.g. `text/plain`.
        content_type: String,
    },
}

/// The unit of content added to and removed from a Session.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextItem {
    /// The payload.
    pub payload: ContextPayload,
    /// Where the item came from. Required.
    pub provenance: Provenance,
    /// Suggested scope. Kept on the item; the scope passed to
    /// [`crate::Session::add`] decides where the item goes.
    pub scope_hint: Option<ContextScope>,
    /// Optional embedding. Stored with the item; similarity search over it is
    /// not yet implemented.
    pub embedding: Option<Vec<f32>>,
}

impl ContextItem {
    /// An item with no scope hint and no embedding.
    pub fn new(payload: ContextPayload, provenance: Provenance) -> Self {
        Self {
            payload,
            provenance,
            scope_hint: None,
            embedding: None,
        }
    }

    /// A [`ContextPayload::Triples`] item.
    pub fn triples(quads: Vec<Quad>, provenance: Provenance) -> Self {
        Self::new(ContextPayload::Triples(quads), provenance)
    }

    /// A [`ContextPayload::Event`] item.
    pub fn event(kind: impl Into<String>, body: serde_json::Value, provenance: Provenance) -> Self {
        Self::new(
            ContextPayload::Event {
                kind: kind.into(),
                body,
            },
            provenance,
        )
    }

    /// A [`ContextPayload::Chunk`] item.
    pub fn chunk(
        content: impl Into<String>,
        content_type: impl Into<String>,
        provenance: Provenance,
    ) -> Self {
        Self::new(
            ContextPayload::Chunk {
                content: content.into(),
                content_type: content_type.into(),
            },
            provenance,
        )
    }

    /// Set the scope hint.
    pub fn with_scope_hint(mut self, scope: ContextScope) -> Self {
        self.scope_hint = Some(scope);
        self
    }

    /// Set the embedding.
    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_tags_round_trip() {
        for s in ContextScope::ALL {
            assert_eq!(ContextScope::from_tag(s.tag()), Some(s));
            assert_eq!(s.to_string(), s.tag());
        }
        assert_eq!(ContextScope::from_tag("nope"), None);
    }

    #[test]
    fn item_id_displays_its_number() {
        assert_eq!(ItemId(7).to_string(), "item:7");
    }
}
