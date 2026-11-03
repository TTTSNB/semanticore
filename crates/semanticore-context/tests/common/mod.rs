//! Shared fixtures for the `semanticore-context` integration tests.

#![allow(dead_code)] // each test binary uses a different subset

use std::path::Path;

use semanticore_context::{
    ContextItem, ContextQuery, ContextScope, ItemId, Provenance, QuadPattern, ScopeSelection,
    Session, SessionConfig,
};
use semanticore_core::model::{NamedNode, Quad, Term};
use semanticore_store_oxigraph::OxigraphStore;
use tempfile::TempDir;

pub fn ex(s: &str) -> NamedNode {
    NamedNode::new(format!("https://example.org/{s}"))
}

pub fn triple(s: &str, p: &str, o: impl Into<Term>) -> Quad {
    Quad::triple(ex(s).into(), ex(p), o.into())
}

pub fn prov() -> Provenance {
    Provenance::new("test")
}

/// A one-triple item: `<s> <p> <o>`.
pub fn fact(s: &str, p: &str, o: &str) -> ContextItem {
    ContextItem::triples(vec![triple(s, p, ex(o))], prov())
}

/// The default OSS config: file-backed enduring tier in `dir`, in-memory
/// major and minor tiers.
pub fn config(dir: &Path) -> SessionConfig<OxigraphStore> {
    SessionConfig::oxigraph(dir).expect("open oxigraph stores")
}

/// A fresh temp dir plus a Session opened over it with `tweak` applied to
/// the default config.
pub fn session_with(
    tweak: impl FnOnce(&mut SessionConfig<OxigraphStore>),
) -> (TempDir, Session<OxigraphStore>) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path());
    tweak(&mut cfg);
    let session = Session::open(cfg).expect("open session");
    (dir, session)
}

pub fn session() -> (TempDir, Session<OxigraphStore>) {
    session_with(|_| {})
}

/// A pattern query matching every triple item in `scope`.
pub fn everything_in(scope: ContextScope) -> ContextQuery {
    ContextQuery::pattern(ScopeSelection::Single(scope), QuadPattern::default())
}

/// A pattern query for subject `<s>` in `scope`.
pub fn subject_in(scope: ContextScope, s: &str) -> ContextQuery {
    ContextQuery::pattern(
        ScopeSelection::Single(scope),
        QuadPattern::default().with_subject(ex(s)),
    )
}

/// The ids returned by a query, in result order.
pub fn ids(session: &Session<OxigraphStore>, q: ContextQuery) -> Vec<ItemId> {
    session
        .query(q)
        .expect("query")
        .items
        .into_iter()
        .map(|r| r.id)
        .collect()
}
