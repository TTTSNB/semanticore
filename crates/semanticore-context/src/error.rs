//! Error types.

use std::fmt;

use semanticore_core::storage::StorageCapability;

use crate::item::{ContextScope, ItemId};

/// Operations the RFC reserves for the premium tier at v0.1. The OSS tier
/// refuses them rather than silently doing nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ContextError {
    /// SHACL validation needs a premium SHACL reasoner.
    #[error("SHACL validation needs a SHACL reasoner from the premium tier")]
    ShaclUnavailable,
    /// Datalog evaluation needs a premium Datalog engine.
    #[error("Datalog evaluation needs a Datalog engine from the premium tier")]
    DatalogUnavailable,
}

/// Parts of RFC 0004 the OSS crate specifies but does not implement yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unimplemented {
    /// The default OWL 2 EL reasoner: [`crate::Session::reason`], inference
    /// queries and query-time reasoner hints.
    OwlElReasoner,
    /// The default ANN (HNSW) index behind similarity queries.
    AnnIndex,
    /// SPARQL query evaluation.
    Sparql,
}

impl fmt::Display for Unimplemented {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Unimplemented::OwlElReasoner => "the default OWL 2 EL reasoner",
            Unimplemented::AnnIndex => "similarity search (default ANN/HNSW index)",
            Unimplemented::Sparql => "SPARQL query evaluation",
        })
    }
}

/// Errors from [`crate::Session`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    /// A scope's storage lacks a capability the scope needs (the enduring
    /// tier needs persistent storage).
    #[error("{scope} storage lacks the {required:?} capability")]
    CapabilityShortfall {
        /// The scope.
        scope: ContextScope,
        /// The missing capability.
        required: StorageCapability,
    },
    /// The item's provenance has an empty source.
    #[error("item has no provenance")]
    MissingProvenance,
    /// The item cannot be stored as given.
    #[error("invalid item: {reason}")]
    InvalidItem {
        /// Why.
        reason: String,
    },
    /// No live item has this id.
    #[error("unknown item {0}")]
    UnknownItem(ItemId),
    /// The item is not in the scope the operation names.
    #[error("{item} is in {found}, not {expected}")]
    ScopeMismatch {
        /// The item.
        item: ItemId,
        /// The scope the operation named.
        expected: ContextScope,
        /// The scope the item is in.
        found: ContextScope,
    },
    /// Promotion to a less durable scope, or demotion to a scope that is not
    /// less durable.
    #[error("cannot move items from {from} to {to} this way")]
    DurabilityViolation {
        /// Source scope.
        from: ContextScope,
        /// Requested target scope.
        to: ContextScope,
    },
    /// The operation is reserved for the premium tier at v0.1.
    #[error("premium-only at v0.1: {0}")]
    PremiumOnly(#[from] ContextError),
    /// The operation is specified but not implemented in this release.
    #[error("not yet implemented: {0}")]
    NotYetImplemented(Unimplemented),
    /// Stored item metadata could not be decoded on open.
    #[error("corrupt context store: {reason}")]
    Corrupt {
        /// Why.
        reason: String,
    },
    /// An error from the underlying storage.
    #[error("storage: {0}")]
    Storage(#[source] Box<dyn std::error::Error + Send + Sync>),
}

pub(crate) fn storage<E: std::error::Error + Send + Sync + 'static>(e: E) -> SessionError {
    SessionError::Storage(Box::new(e))
}
