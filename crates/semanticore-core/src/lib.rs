//! Shared types and traits for the semantiCore Rust ecosystem.
//!
//! `semanticore-core` is the OSS crate that **defines** every cross-cutting trait
//! in the workspace; OSS default impls live in `semanticore-context`, and premium
//! impls live in the `semanticore-reasoner-*` / `semanticore-context-engine-*`
//! crates. Per RFC 0001, premium crates implement these traits and never define
//! them.
//!
//! Modules:
//! - [`model`] — the canonical RDF term model ([`model::Quad`] and friends).
//! - [`storage`] — the [`storage::Storage`] / [`storage::StorageTransaction`]
//!   traits (RFC 0002).
//! - [`reasoner`] — the [`reasoner::Reasoner`] base trait and capability
//!   advertisement (RFC 0003).
//! - [`shacl`] — the [`shacl::ShaclValidator`] trait and report vocabulary
//!   (RFC 0003).
//! - [`datalog`] — the [`datalog::DatalogEngine`] trait, program IR, and result
//!   types (RFC 0003).
//!
//! Scope: this is the trait layer the SHACL + Datalog premium engines compile
//! against. The OWL and Context reasoner families from RFC 0003 land in the
//! remainder of Phase 3.

#![doc(html_root_url = "https://docs.rs/semanticore-core")]

pub mod datalog;
pub mod model;
pub mod reasoner;
pub mod shacl;
pub mod storage;

/// An IRI. Retained from the initial scaffold; new code should prefer
/// [`model::NamedNode`] for IRI-named RDF nodes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Iri(String);

impl Iri {
    /// Construct an IRI.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The IRI as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iri_round_trip() {
        let iri = Iri::new("https://example.org/foo");
        assert_eq!(iri.as_str(), "https://example.org/foo");
    }
}
