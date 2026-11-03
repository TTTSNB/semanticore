//! `ContextQuery`, `ContextQueryResult`, `RankedItem` and `QueryStats`.

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use semanticore_context as ctx;

use crate::item::{PyContextItem, PyContextScope, PyItemId};
use crate::terms::{object_from_py, predicate_from_py, subject_from_py};

/// Which scopes a query consults, from its Python spelling:
///
/// - `None` -> all three, merged most durable first;
/// - a `ContextScope` -> that scope alone;
/// - an iterable of `ContextScope` -> those scopes, in order, once each.
pub fn scopes_from_py(obj: Option<&Bound<'_, PyAny>>) -> PyResult<ctx::ScopeSelection> {
    let Some(obj) = obj.filter(|o| !o.is_none()) else {
        return Ok(ctx::ScopeSelection::All);
    };
    if let Ok(s) = obj.extract::<PyContextScope>() {
        return Ok(ctx::ScopeSelection::Single(s.into()));
    }
    let err = || PyTypeError::new_err("scopes must be None, a ContextScope or an iterable of them");
    let iter = obj.try_iter().map_err(|_| err())?;
    let scopes = iter
        .map(|s| {
            s?.extract::<PyContextScope>()
                .map(Into::into)
                .map_err(|_| err())
        })
        .collect::<PyResult<Vec<_>>>()?;
    Ok(ctx::ScopeSelection::Set(scopes))
}

fn pattern_from_py(
    subject: Option<&Bound<'_, PyAny>>,
    predicate: Option<&Bound<'_, PyAny>>,
    object: Option<&Bound<'_, PyAny>>,
) -> PyResult<ctx::QuadPattern> {
    fn given<'a, 'py>(o: Option<&'a Bound<'py, PyAny>>) -> Option<&'a Bound<'py, PyAny>> {
        o.filter(|o| !o.is_none())
    }
    Ok(ctx::QuadPattern {
        subject: given(subject).map(subject_from_py).transpose()?,
        predicate: given(predicate).map(predicate_from_py).transpose()?,
        object: given(object).map(object_from_py).transpose()?,
    })
}

/// A query against a Session. Build one with `pattern`, `sparql`,
/// `similarity` or `inference`.
#[pyclass(name = "ContextQuery", module = "semanticore_context", frozen, eq)]
#[derive(Debug, Clone, PartialEq)]
pub struct PyContextQuery(pub ctx::ContextQuery);

fn build(
    scopes: Option<&Bound<'_, PyAny>>,
    kind: ctx::ContextQueryKind,
    limit: Option<usize>,
    reasoner_hint: Option<PyContextScope>,
) -> PyResult<PyContextQuery> {
    let mut q = ctx::ContextQuery::new(scopes_from_py(scopes)?, kind);
    q.limit = limit;
    q.reasoner_hint = reasoner_hint.map(Into::into);
    Ok(PyContextQuery(q))
}

#[pymethods]
impl PyContextQuery {
    /// Match items whose triples match a pattern. `None` matches anything.
    #[staticmethod]
    #[pyo3(signature = (subject = None, predicate = None, object = None, *, scopes = None, limit = None, reasoner_hint = None))]
    fn pattern(
        subject: Option<&Bound<'_, PyAny>>,
        predicate: Option<&Bound<'_, PyAny>>,
        object: Option<&Bound<'_, PyAny>>,
        scopes: Option<&Bound<'_, PyAny>>,
        limit: Option<usize>,
        reasoner_hint: Option<PyContextScope>,
    ) -> PyResult<Self> {
        let pattern = pattern_from_py(subject, predicate, object)?;
        build(
            scopes,
            ctx::ContextQueryKind::Pattern(pattern),
            limit,
            reasoner_hint,
        )
    }

    /// A SPARQL query string. Not yet implemented.
    #[staticmethod]
    #[pyo3(signature = (query, *, scopes = None, limit = None, reasoner_hint = None))]
    fn sparql(
        query: String,
        scopes: Option<&Bound<'_, PyAny>>,
        limit: Option<usize>,
        reasoner_hint: Option<PyContextScope>,
    ) -> PyResult<Self> {
        build(
            scopes,
            ctx::ContextQueryKind::Sparql(query),
            limit,
            reasoner_hint,
        )
    }

    /// Similarity search over embeddings. Not yet implemented.
    #[staticmethod]
    #[pyo3(signature = (query_embedding, top_k, *, scopes = None, limit = None, reasoner_hint = None))]
    fn similarity(
        query_embedding: Vec<f32>,
        top_k: usize,
        scopes: Option<&Bound<'_, PyAny>>,
        limit: Option<usize>,
        reasoner_hint: Option<PyContextScope>,
    ) -> PyResult<Self> {
        build(
            scopes,
            ctx::ContextQueryKind::Similarity {
                query_embedding,
                top_k,
            },
            limit,
            reasoner_hint,
        )
    }

    /// Reasoner-augmented inference over a pattern. Not yet implemented.
    #[staticmethod]
    #[pyo3(signature = (subject = None, predicate = None, object = None, *, scopes = None, limit = None, reasoner_hint = None))]
    fn inference(
        subject: Option<&Bound<'_, PyAny>>,
        predicate: Option<&Bound<'_, PyAny>>,
        object: Option<&Bound<'_, PyAny>>,
        scopes: Option<&Bound<'_, PyAny>>,
        limit: Option<usize>,
        reasoner_hint: Option<PyContextScope>,
    ) -> PyResult<Self> {
        let pattern = pattern_from_py(subject, predicate, object)?;
        build(
            scopes,
            ctx::ContextQueryKind::Inference(ctx::InferenceQuery { pattern }),
            limit,
            reasoner_hint,
        )
    }

    /// A copy capped at `limit` results.
    fn with_limit(&self, limit: usize) -> Self {
        Self(self.0.clone().with_limit(limit))
    }

    /// A copy that asks for query-time reasoning under `scope`'s reasoner.
    fn with_reasoner_hint(&self, scope: PyContextScope) -> Self {
        Self(self.0.clone().with_reasoner_hint(scope.into()))
    }

    /// `"pattern"`, `"sparql"`, `"similarity"` or `"inference"`.
    #[getter]
    fn kind(&self) -> &'static str {
        match &self.0.kind {
            ctx::ContextQueryKind::Pattern(_) => "pattern",
            ctx::ContextQueryKind::Sparql(_) => "sparql",
            ctx::ContextQueryKind::Similarity { .. } => "similarity",
            ctx::ContextQueryKind::Inference(_) => "inference",
            // `ContextQueryKind` is non-exhaustive.
            _ => "unknown",
        }
    }

    /// `None` (all scopes), a `ContextScope`, or a list of them; the same
    /// spellings `scopes=` accepts.
    #[getter]
    fn scopes<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        Ok(match &self.0.scopes {
            ctx::ScopeSelection::All => None,
            ctx::ScopeSelection::Single(s) => {
                Some(Bound::new(py, PyContextScope::from(*s))?.into_any())
            }
            ctx::ScopeSelection::Set(v) => {
                let v: Vec<PyContextScope> = v.iter().copied().map(Into::into).collect();
                Some(v.into_pyobject(py)?.into_any())
            }
        })
    }

    #[getter]
    fn limit(&self) -> Option<usize> {
        self.0.limit
    }

    #[getter]
    fn reasoner_hint(&self) -> Option<PyContextScope> {
        self.0.reasoner_hint.map(Into::into)
    }

    fn __repr__(&self) -> String {
        format!("<ContextQuery {} limit={:?}>", self.kind(), self.0.limit)
    }
}

/// One result of a query.
#[pyclass(name = "RankedItem", module = "semanticore_context", frozen, eq)]
#[derive(Debug, Clone, PartialEq)]
pub struct PyRankedItem(pub ctx::RankedItem);

#[pymethods]
impl PyRankedItem {
    #[getter]
    fn id(&self) -> PyItemId {
        PyItemId(self.0.id)
    }

    #[getter]
    fn item(&self) -> PyContextItem {
        PyContextItem(self.0.item.clone())
    }

    /// Relevance; 1.0 for every pattern match.
    #[getter]
    fn score(&self) -> f64 {
        self.0.score
    }

    /// The scope that served the item.
    #[getter]
    fn source_scope(&self) -> PyContextScope {
        self.0.source_scope.into()
    }

    fn __repr__(&self) -> String {
        format!(
            "<RankedItem {} score={} from {}>",
            self.0.id, self.0.score, self.0.source_scope
        )
    }
}

/// Counters for one query.
#[pyclass(
    name = "QueryStats",
    module = "semanticore_context",
    frozen,
    eq,
    get_all
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PyQueryStats {
    /// Distinct scopes consulted.
    pub scopes_consulted: usize,
    /// Item quads that matched, before de-duplication and the limit.
    pub quads_matched: u64,
    /// Items auto-promoted at the end of the query.
    pub auto_promoted: u64,
}

impl From<ctx::QueryStats> for PyQueryStats {
    fn from(s: ctx::QueryStats) -> Self {
        Self {
            scopes_consulted: s.scopes_consulted,
            quads_matched: s.quads_matched,
            auto_promoted: s.auto_promoted,
        }
    }
}

#[pymethods]
impl PyQueryStats {
    fn __repr__(&self) -> String {
        format!("{self:?}").replacen("PyQueryStats", "QueryStats", 1)
    }
}

/// The result of `Session.query`.
#[pyclass(
    name = "ContextQueryResult",
    module = "semanticore_context",
    frozen,
    eq
)]
#[derive(Debug, Clone, PartialEq)]
pub struct PyContextQueryResult(pub ctx::ContextQueryResult);

#[pymethods]
impl PyContextQueryResult {
    /// The results, highest score first.
    #[getter]
    fn items(&self) -> Vec<PyRankedItem> {
        self.0.items.iter().cloned().map(PyRankedItem).collect()
    }

    #[getter]
    fn query_stats(&self) -> PyQueryStats {
        self.0.query_stats.into()
    }

    fn __repr__(&self) -> String {
        format!("<ContextQueryResult {} item(s)>", self.0.items.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::ffi::c_str;
    use semanticore_core::model::{Literal, NamedNode};

    fn scope_ns(py: Python<'_>) -> Bound<'_, pyo3::types::PyDict> {
        let d = pyo3::types::PyDict::new(py);
        d.set_item("ContextScope", py.get_type::<PyContextScope>())
            .unwrap();
        d
    }

    #[test]
    fn scope_spellings_map_to_scope_selections() {
        Python::attach(|py| {
            let ns = scope_ns(py);
            let ev = |c: &std::ffi::CStr| py.eval(c, Some(&ns), None).unwrap();
            assert_eq!(scopes_from_py(None).unwrap(), ctx::ScopeSelection::All);
            assert_eq!(
                scopes_from_py(Some(&ev(c_str!("None")))).unwrap(),
                ctx::ScopeSelection::All
            );
            assert_eq!(
                scopes_from_py(Some(&ev(c_str!("ContextScope.MAJOR")))).unwrap(),
                ctx::ScopeSelection::Single(ctx::ContextScope::Major)
            );
            assert_eq!(
                scopes_from_py(Some(&ev(c_str!(
                    "(ContextScope.MINOR, ContextScope.ENDURING)"
                ))))
                .unwrap(),
                ctx::ScopeSelection::Set(vec![
                    ctx::ContextScope::Minor,
                    ctx::ContextScope::Enduring
                ])
            );
            for bad in [
                c_str!("'major'"),
                c_str!("[ContextScope.MAJOR, 1]"),
                c_str!("3"),
            ] {
                assert!(scopes_from_py(Some(&ev(bad)))
                    .unwrap_err()
                    .is_instance_of::<PyTypeError>(py));
            }
        });
    }

    #[test]
    fn pattern_builder_fills_the_rust_query() {
        Python::attach(|py| {
            let s = pyo3::types::PyString::new(py, "urn:alice").into_any();
            let o = Bound::new(py, crate::terms::PyLiteral(Literal::new_simple("A")))
                .unwrap()
                .into_any();
            let q = PyContextQuery::pattern(
                Some(&s),
                None,
                Some(&o),
                None,
                Some(2),
                Some(PyContextScope::Major),
            )
            .unwrap();
            let expected = ctx::ContextQuery::pattern(
                ctx::ScopeSelection::All,
                ctx::QuadPattern::default()
                    .with_subject(NamedNode::new("urn:alice"))
                    .with_object(Literal::new_simple("A")),
            )
            .with_limit(2)
            .with_reasoner_hint(ctx::ContextScope::Major);
            assert_eq!(q.0, expected);
            assert_eq!(q.kind(), "pattern");
            assert!(q.scopes(py).unwrap().is_none());
        });
    }

    #[test]
    fn other_query_kinds_build_their_rust_variants() {
        let q = PyContextQuery::sparql("ASK {}".into(), None, None, None).unwrap();
        assert_eq!(q.0.kind, ctx::ContextQueryKind::Sparql("ASK {}".into()));
        let q = PyContextQuery::similarity(vec![1.0], 3, None, None, None).unwrap();
        assert_eq!(
            q.0.kind,
            ctx::ContextQueryKind::Similarity {
                query_embedding: vec![1.0],
                top_k: 3
            }
        );
        let q = PyContextQuery::inference(None, None, None, None, None, None).unwrap();
        assert_eq!(q.kind(), "inference");
        let q = q.with_limit(4).with_reasoner_hint(PyContextScope::Minor);
        assert_eq!(q.0.limit, Some(4));
        assert_eq!(q.0.reasoner_hint, Some(ctx::ContextScope::Minor));
    }
}
