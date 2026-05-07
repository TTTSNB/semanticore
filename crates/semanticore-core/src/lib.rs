//! Shared types and traits for the semantiCore Rust ecosystem.
//!
//! This crate is currently a placeholder. The full type system lands in task T3.1
//! of the semantiCore Rebuild plan.

#![doc(html_root_url = "https://docs.rs/semanticore-core")]

/// Placeholder type — full IRI implementation lands in T3.1.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Iri(String);

impl Iri {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

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
