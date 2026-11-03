//! The semantiCore Context OS (RFC 0004): a [`Session`] over three context
//! tiers.
//!
//! | Scope | Lifetime | OSS default storage |
//! |---|---|---|
//! | [`ContextScope::Enduring`] | survives the Session | file-backed Oxigraph |
//! | [`ContextScope::Major`] | the Session | in-memory Oxigraph |
//! | [`ContextScope::Minor`] | the Session (meant for one turn) | in-memory Oxigraph |
//!
//! ```
//! use semanticore_context::{
//!     ContextItem, ContextQuery, ContextScope, Provenance, QuadPattern, ScopeSelection, Session,
//!     SessionConfig,
//! };
//! use semanticore_core::model::{NamedNode, Quad};
//!
//! let dir = tempfile::tempdir()?;
//! let mut session = Session::open(SessionConfig::oxigraph(dir.path())?)?;
//!
//! let ex = |s: &str| NamedNode::new(format!("https://example.org/{s}"));
//! let fact = Quad::triple(ex("alice").into(), ex("knows"), ex("bob").into());
//! let id = session.add(
//!     ContextScope::Major,
//!     ContextItem::triples(vec![fact], Provenance::new("chat")),
//! )?;
//!
//! let result = session.query(ContextQuery::pattern(
//!     ScopeSelection::All,
//!     QuadPattern::default().with_subject(ex("alice")),
//! ))?;
//! assert_eq!(result.items[0].id, id);
//!
//! // The user confirmed it: keep it beyond this Session.
//! session.promote(&[id], ContextScope::Major, ContextScope::Enduring)?;
//! session.close()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Lifecycle rules
//!
//! - **Explicit promotion** ([`Session::promote`]) moves items to an equal or
//!   more durable scope.
//! - **Auto-promotion** runs after each query ([`PromotionConfig`]): a minor
//!   item returned by more than 3 queries within 60 s moves to major; a major
//!   item cited by more than 5 other items moves to enduring. An item moves at
//!   most one tier per query.
//! - **Eviction** ([`Session::evict`], or automatically when a tier exceeds
//!   its [`ScopeBounds`]) picks victims by [`EvictionPolicy`] and drops or
//!   demotes them per [`DemotionTarget`]. Explicit demotion is
//!   [`EvictionCriteria::Demote`].
//!
//! # What v0.1 does not do yet
//!
//! These are specified by RFC 0004 and return typed errors instead of
//! silently doing nothing:
//!
//! - Premium-only: [`Session::validate`] (SHACL) and
//!   [`Session::evaluate_datalog`] return [`SessionError::PremiumOnly`].
//! - Not yet implemented ([`SessionError::NotYetImplemented`]): the default
//!   OWL 2 EL reasoner ([`Session::reason`], inference queries, reasoner
//!   hints), the default ANN/HNSW index (similarity queries) and SPARQL
//!   queries.

#![doc(html_root_url = "https://docs.rs/semanticore-context")]

mod config;
mod error;
mod item;
mod query;
mod record;
mod session;

pub use config::{
    DemotionTarget, EvictionCriteria, EvictionPolicy, FlushStrategy, PersistenceConfig,
    PersistenceTarget, PromotionCadence, PromotionConfig, ReasonerSelection, ScopeBounds,
    SessionConfig, StorageConfig,
};
pub use error::{ContextError, SessionError, Unimplemented};
pub use item::{ContextItem, ContextPayload, ContextScope, ItemId, Provenance};
pub use query::{
    ContextQuery, ContextQueryKind, ContextQueryResult, InferenceQuery, QuadPattern, QueryStats,
    RankedItem, ScopeSelection,
};
pub use session::{ContextSnapshot, PromotionStats, ReasoningStats, Session, SessionStats};
