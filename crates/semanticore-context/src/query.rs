//! The query DSL: `ContextQuery` and `ContextQueryResult`.

use semanticore_core::model::{NamedNode, NamedOrBlankNode, Term};

use crate::item::{ContextItem, ContextScope, ItemId};

/// A triple pattern over items' triples. `None` matches anything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuadPattern {
    /// Subject to match.
    pub subject: Option<NamedOrBlankNode>,
    /// Predicate to match.
    pub predicate: Option<NamedNode>,
    /// Object to match.
    pub object: Option<Term>,
}

impl QuadPattern {
    /// Match this subject.
    pub fn with_subject(mut self, subject: impl Into<NamedOrBlankNode>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Match this predicate.
    pub fn with_predicate(mut self, predicate: NamedNode) -> Self {
        self.predicate = Some(predicate);
        self
    }

    /// Match this object.
    pub fn with_object(mut self, object: impl Into<Term>) -> Self {
        self.object = Some(object.into());
        self
    }
}

/// Which scopes a query consults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeSelection {
    /// One scope.
    Single(ContextScope),
    /// All three, merged in durability order (enduring, major, minor).
    All,
    /// The listed scopes, in the order given; repeats are consulted once.
    Set(Vec<ContextScope>),
}

impl ScopeSelection {
    pub(crate) fn scopes(&self) -> Vec<ContextScope> {
        let listed: &[ContextScope] = match self {
            ScopeSelection::Single(s) => std::slice::from_ref(s),
            ScopeSelection::All => &ContextScope::ALL,
            ScopeSelection::Set(v) => v,
        };
        let mut out = Vec::with_capacity(3);
        for s in listed {
            if !out.contains(s) {
                out.push(*s);
            }
        }
        out
    }
}

/// A reasoner-augmented query: ask the scope's reasoner to entail answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceQuery {
    /// The pattern whose entailed matches are wanted.
    pub pattern: QuadPattern,
}

/// What a [`ContextQuery`] asks.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ContextQueryKind {
    /// Match items whose triples match a pattern. Score 1.0 per hit.
    Pattern(QuadPattern),
    /// A SPARQL query string. Not yet implemented.
    Sparql(String),
    /// Similarity search over embeddings. Not yet implemented (needs the ANN
    /// index).
    Similarity {
        /// The query vector.
        query_embedding: Vec<f32>,
        /// How many results to return.
        top_k: usize,
    },
    /// Reasoner-augmented inference. Not yet implemented (needs the OWL 2 EL
    /// reasoner).
    Inference(InferenceQuery),
}

/// A query against a Session.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextQuery {
    /// Which scopes to consult and how to merge them.
    pub scopes: ScopeSelection,
    /// The query itself.
    pub kind: ContextQueryKind,
    /// Maximum results to return, after merging.
    pub limit: Option<usize>,
    /// Apply this scope's reasoner at query time. Not yet implemented: any
    /// `Some` is refused.
    pub reasoner_hint: Option<ContextScope>,
}

impl ContextQuery {
    /// A query with no limit and no reasoner hint.
    pub fn new(scopes: ScopeSelection, kind: ContextQueryKind) -> Self {
        Self {
            scopes,
            kind,
            limit: None,
            reasoner_hint: None,
        }
    }

    /// A [`ContextQueryKind::Pattern`] query.
    pub fn pattern(scopes: ScopeSelection, pattern: QuadPattern) -> Self {
        Self::new(scopes, ContextQueryKind::Pattern(pattern))
    }

    /// Cap the number of results.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Ask for query-time reasoning under `scope`'s reasoner.
    pub fn with_reasoner_hint(mut self, scope: ContextScope) -> Self {
        self.reasoner_hint = Some(scope);
        self
    }
}

/// One result of a query.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedItem {
    /// The item's handle.
    pub id: ItemId,
    /// The item.
    pub item: ContextItem,
    /// Relevance; 1.0 for every pattern match.
    pub score: f64,
    /// The scope that served the item. If the query auto-promoted the item,
    /// this is the scope it was served from, not the one it moved to.
    pub source_scope: ContextScope,
}

/// Counters for one query.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueryStats {
    /// Distinct scopes consulted.
    pub scopes_consulted: usize,
    /// Item quads that matched, before de-duplication by item and before
    /// the limit.
    pub quads_matched: u64,
    /// Items auto-promoted at the end of this query.
    pub auto_promoted: u64,
}

/// The result of [`crate::Session::query`].
#[derive(Debug, Clone, PartialEq)]
pub struct ContextQueryResult {
    /// The results, highest score first; ties in scope-selection order, then
    /// oldest item first.
    pub items: Vec<RankedItem>,
    /// Counters.
    pub query_stats: QueryStats,
}
