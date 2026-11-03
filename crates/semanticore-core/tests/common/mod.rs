//! Shared test support: proptest strategies for the RDF term model and a
//! test-only in-memory `Storage` implementation.
//!
//! `semanticore-core` ships the storage *traits* only (see CHANGELOG 0.1.0);
//! there is no backend in this repository. `MemStore` exists so the RFC 0002
//! contract can be expressed as executable properties. The contract checks in
//! `tests/storage_contract.rs` are written generically over `S: Storage`, so a
//! real adapter (e.g. the Oxigraph backend) can reuse them unchanged.
//!
//! Not every integration-test binary uses every helper here.
#![allow(dead_code)]

use std::collections::HashSet;
use std::sync::Mutex;

use proptest::prelude::*;
use semanticore_core::model::{
    xsd, BlankNode, Literal, NamedNode, NamedOrBlankNode, NamedOrDefault, Quad, Term,
};
use semanticore_core::storage::{
    Storage, StorageCapabilities, StorageError, StorageTransaction, TransactionIsolation,
};

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------
//
// Two tiers:
// - `small_*`: tiny alphanumeric alphabets so generated values collide often.
//   This is what makes idempotency / duplicate / equality properties bite.
//   No string they produce contains `<`, `>` or `"`, so Display output is
//   unambiguous (see `display_is_injective_on_safe_alphabet`).
// - `any_*`: arbitrary Unicode strings, for properties that must hold for
//   every input (accessor round trips, Eq/Hash consistency).

pub fn small_iri() -> impl Strategy<Value = String> {
    "[a-c]{1,2}".prop_map(|s| format!("https://example.org/{s}"))
}

pub fn small_named() -> impl Strategy<Value = NamedNode> {
    small_iri().prop_map(NamedNode::new)
}

pub fn small_blank() -> impl Strategy<Value = BlankNode> {
    "[a-c]{1,2}".prop_map(BlankNode::new)
}

/// Datatypes for typed literals. Deliberately excludes `rdf:langString`;
/// see `tests/regressions.rs` for why.
pub fn small_datatype() -> impl Strategy<Value = NamedNode> {
    prop_oneof![
        Just(NamedNode::new(xsd::STRING)),
        Just(NamedNode::new("http://www.w3.org/2001/XMLSchema#integer")),
        small_named(),
    ]
}

/// Well-formed literals: simple, typed (non-langString), or with a non-empty
/// language tag.
pub fn small_literal() -> impl Strategy<Value = Literal> {
    prop_oneof![
        "[a-c ]{0,3}".prop_map(Literal::new_simple),
        ("[a-c ]{0,3}", small_datatype()).prop_map(|(v, d)| Literal::new_typed(v, d)),
        ("[a-c ]{0,3}", "(en|fr|en-GB)").prop_map(|(v, l)| Literal::new_lang(v, l)),
    ]
}

pub fn small_subject() -> impl Strategy<Value = NamedOrBlankNode> {
    prop_oneof![
        small_named().prop_map(NamedOrBlankNode::Named),
        small_blank().prop_map(NamedOrBlankNode::Blank),
    ]
}

pub fn small_term() -> impl Strategy<Value = Term> {
    prop_oneof![
        small_named().prop_map(Term::Named),
        small_blank().prop_map(Term::Blank),
        small_literal().prop_map(Term::Literal),
    ]
}

pub fn small_graph() -> impl Strategy<Value = NamedOrDefault> {
    prop_oneof![
        Just(NamedOrDefault::Default),
        small_named().prop_map(NamedOrDefault::Named),
    ]
}

pub fn small_quad() -> impl Strategy<Value = Quad> {
    (small_subject(), small_named(), small_term(), small_graph())
        .prop_map(|(s, p, o, g)| Quad::new(s, p, o, g))
}

pub fn small_quads(max: usize) -> impl Strategy<Value = Vec<Quad>> {
    prop::collection::vec(small_quad(), 0..max)
}

pub fn any_named() -> impl Strategy<Value = NamedNode> {
    any::<String>().prop_map(NamedNode::new)
}

pub fn any_blank() -> impl Strategy<Value = BlankNode> {
    any::<String>().prop_map(BlankNode::new)
}

pub fn any_literal() -> impl Strategy<Value = Literal> {
    prop_oneof![
        any::<String>().prop_map(Literal::new_simple),
        (any::<String>(), any_named()).prop_map(|(v, d)| Literal::new_typed(v, d)),
        (any::<String>(), any::<String>()).prop_map(|(v, l)| Literal::new_lang(v, l)),
    ]
}

pub fn any_term() -> impl Strategy<Value = Term> {
    prop_oneof![
        any_named().prop_map(Term::Named),
        any_blank().prop_map(Term::Blank),
        any_literal().prop_map(Term::Literal),
    ]
}

/// A pattern position: `None` (wildcard) or `Some` drawn from `inner`.
pub fn maybe<T: std::fmt::Debug + Clone>(
    inner: impl Strategy<Value = T>,
) -> impl Strategy<Value = Option<T>> {
    prop_oneof![Just(None), inner.prop_map(Some)]
}

// ---------------------------------------------------------------------------
// Test-only in-memory store
// ---------------------------------------------------------------------------

#[derive(Default)]
struct State {
    /// Bumped on every successful write commit; used for optimistic
    /// concurrency control so `TransactionConflict` is exercised.
    version: u64,
    quads: HashSet<Quad>,
}

/// A minimal snapshot-isolated, in-memory quad store. Test double only.
#[derive(Default)]
pub struct MemStore {
    state: Mutex<State>,
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convenience: insert `quads` in one committed write transaction.
    pub fn with_quads(quads: &[Quad]) -> Self {
        let store = Self::new();
        let mut tx = store.begin_write().unwrap();
        for q in quads {
            tx.insert(q).unwrap();
        }
        tx.commit().unwrap();
        store
    }

    /// The committed state, for assertions.
    pub fn snapshot(&self) -> HashSet<Quad> {
        self.state.lock().unwrap().quads.clone()
    }
}

/// A transaction over a private copy of the committed state.
pub struct MemTx<'a> {
    store: &'a MemStore,
    base_version: u64,
    working: HashSet<Quad>,
    writable: bool,
}

fn read_only_error() -> StorageError {
    StorageError::Backend("write attempted in a read-only transaction".into())
}

impl Storage for MemStore {
    type Transaction<'a> = MemTx<'a>;
    type Error = StorageError;

    fn begin_read(&self) -> Result<MemTx<'_>, StorageError> {
        let st = self.state.lock().unwrap();
        Ok(MemTx {
            store: self,
            base_version: st.version,
            working: st.quads.clone(),
            writable: false,
        })
    }

    fn begin_write(&self) -> Result<MemTx<'_>, StorageError> {
        let mut tx = self.begin_read()?;
        tx.writable = true;
        Ok(tx)
    }

    fn quad_count(&self) -> Result<u64, StorageError> {
        Ok(self.state.lock().unwrap().quads.len() as u64)
    }

    fn named_graphs(&self) -> Result<Vec<NamedNode>, StorageError> {
        let st = self.state.lock().unwrap();
        let mut graphs: Vec<NamedNode> = st
            .quads
            .iter()
            .filter_map(|q| match &q.graph {
                NamedOrDefault::Named(g) => Some(g.clone()),
                NamedOrDefault::Default => None,
            })
            .collect();
        graphs.sort();
        graphs.dedup();
        Ok(graphs)
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities {
            persistent: false,
            concurrent_writes: false,
            bulk_load: false,
            sparql_endpoint: false,
            approximate_quad_count: false,
            isolation: TransactionIsolation::SnapshotIsolation,
        }
    }
}

impl StorageTransaction for MemTx<'_> {
    type Error = StorageError;

    fn insert(&mut self, quad: &Quad) -> Result<bool, StorageError> {
        if !self.writable {
            return Err(read_only_error());
        }
        Ok(self.working.insert(quad.clone()))
    }

    fn remove(&mut self, quad: &Quad) -> Result<bool, StorageError> {
        if !self.writable {
            return Err(read_only_error());
        }
        Ok(self.working.remove(quad))
    }

    fn contains(&self, quad: &Quad) -> Result<bool, StorageError> {
        Ok(self.working.contains(quad))
    }

    fn quads_for_pattern<'a>(
        &'a self,
        s: Option<&NamedOrBlankNode>,
        p: Option<&NamedNode>,
        o: Option<&Term>,
        g: Option<&NamedOrDefault>,
    ) -> Box<dyn Iterator<Item = Result<Quad, StorageError>> + 'a> {
        let (s, p, o, g) = (s.cloned(), p.cloned(), o.cloned(), g.cloned());
        Box::new(
            self.working
                .iter()
                .filter(move |q| matches_pattern(q, s.as_ref(), p.as_ref(), o.as_ref(), g.as_ref()))
                .cloned()
                .map(Ok),
        )
    }

    fn commit(self) -> Result<(), StorageError> {
        if !self.writable {
            return Ok(());
        }
        let mut st = self.store.state.lock().unwrap();
        if st.version != self.base_version {
            return Err(StorageError::TransactionConflict);
        }
        st.quads = self.working;
        st.version += 1;
        Ok(())
    }

    fn rollback(self) -> Result<(), StorageError> {
        Ok(())
    }
}

/// Reference semantics for a quad pattern: `None` matches anything.
pub fn matches_pattern(
    q: &Quad,
    s: Option<&NamedOrBlankNode>,
    p: Option<&NamedNode>,
    o: Option<&Term>,
    g: Option<&NamedOrDefault>,
) -> bool {
    s.is_none_or(|s| &q.subject == s)
        && p.is_none_or(|p| &q.predicate == p)
        && o.is_none_or(|o| &q.object == o)
        && g.is_none_or(|g| &q.graph == g)
}
