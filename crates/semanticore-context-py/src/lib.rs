//! Python binding for `semanticore-context` (RFC 0004 "Bindings story").
//!
//! Built with maturin into the `semanticore_context` extension module:
//!
//! ```python
//! from semanticore_context import ContextItem, ContextQuery, ContextScope, Session, SessionConfig
//!
//! with Session.open(SessionConfig.oxigraph("data/enduring")) as session:
//!     item = ContextItem.from_triples([("urn:alice", "urn:knows", "urn:bob")], "chat")
//!     item_id = session.add(ContextScope.MAJOR, item)
//!     for ranked in session.query(ContextQuery.pattern("urn:alice")).items:
//!         print(ranked.score, ranked.item)
//! ```
//!
//! The binding is marshaling only (RFC 0001 "Dependency rules"): every
//! behavior lives in `semanticore-context`. The surface is synchronous;
//! calls into the native session release the GIL.
//!
//! Modules:
//!
//! - [`terms`]: RDF terms (`str` IRIs, `BlankNode`, `Literal`) and JSON
//!   event bodies.
//! - [`item`]: `ContextScope`, `ItemId`, `Provenance`, `ContextItem`.
//! - [`query`]: `ContextQuery`, `ContextQueryResult`, `RankedItem`,
//!   `QueryStats`.
//! - [`config`]: `SessionConfig` and its parts, `EvictionCriteria`.
//! - [`session`]: `Session` and its stats types.
//! - [`errors`]: the exception hierarchy.

use pyo3::prelude::*;

pub mod config;
pub mod errors;
pub mod item;
pub mod query;
pub mod session;
pub mod terms;

/// The `semanticore_context` Python module.
#[pymodule]
#[pyo3(name = "semanticore_context")]
fn semanticore_context_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;

    m.add_class::<terms::PyBlankNode>()?;
    m.add_class::<terms::PyLiteral>()?;

    m.add_class::<item::PyContextScope>()?;
    m.add_class::<item::PyItemId>()?;
    m.add_class::<item::PyProvenance>()?;
    m.add_class::<item::PyContextItem>()?;

    m.add_class::<query::PyContextQuery>()?;
    m.add_class::<query::PyContextQueryResult>()?;
    m.add_class::<query::PyRankedItem>()?;
    m.add_class::<query::PyQueryStats>()?;

    m.add_class::<config::PySessionConfig>()?;
    m.add_class::<config::PyScopeBounds>()?;
    m.add_class::<config::PyPromotionConfig>()?;
    m.add_class::<config::PyPromotionCadence>()?;
    m.add_class::<config::PyEvictionPolicy>()?;
    m.add_class::<config::PyDemotionTarget>()?;
    m.add_class::<config::PyEvictionCriteria>()?;

    m.add_class::<session::PySession>()?;
    m.add_class::<session::PySessionStats>()?;
    m.add_class::<session::PyPromotionStats>()?;
    m.add_class::<session::PyReasoningStats>()?;
    m.add_class::<session::PyContextSnapshot>()?;

    errors::register(m)?;
    Ok(())
}
