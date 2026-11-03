//! `ContextScope`, `ItemId`, `Provenance` and `ContextItem`.

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyString};
use semanticore_context as ctx;

use crate::terms::{json_from_py, json_to_py, triple_from_py, triple_to_py};

/// The three context tiers, ordered by durability:
/// `MINOR < MAJOR < ENDURING`.
#[pyclass(
    name = "ContextScope",
    module = "semanticore_context",
    frozen,
    eq,
    ord,
    hash,
    from_py_object
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PyContextScope {
    /// Turn-lived, ephemeral context.
    #[pyo3(name = "MINOR")]
    Minor,
    /// Session-lived working set.
    #[pyo3(name = "MAJOR")]
    Major,
    /// Persistent context that survives the Session.
    #[pyo3(name = "ENDURING")]
    Enduring,
}

impl From<ctx::ContextScope> for PyContextScope {
    fn from(s: ctx::ContextScope) -> Self {
        match s {
            ctx::ContextScope::Minor => Self::Minor,
            ctx::ContextScope::Major => Self::Major,
            ctx::ContextScope::Enduring => Self::Enduring,
        }
    }
}

impl From<PyContextScope> for ctx::ContextScope {
    fn from(s: PyContextScope) -> Self {
        match s {
            PyContextScope::Minor => Self::Minor,
            PyContextScope::Major => Self::Major,
            PyContextScope::Enduring => Self::Enduring,
        }
    }
}

#[pymethods]
impl PyContextScope {
    /// Every scope, most durable first.
    #[classattr]
    #[pyo3(name = "ALL")]
    fn all() -> Vec<PyContextScope> {
        ctx::ContextScope::ALL.into_iter().map(Into::into).collect()
    }

    /// The next less durable scope, or `None` for `MINOR`.
    fn colder(&self) -> Option<PyContextScope> {
        ctx::ContextScope::from(*self).colder().map(Into::into)
    }

    fn __str__(&self) -> String {
        ctx::ContextScope::from(*self).to_string()
    }
}

/// An opaque handle to an item, stable for the lifetime of its Session.
#[pyclass(
    name = "ItemId",
    module = "semanticore_context",
    frozen,
    eq,
    ord,
    hash,
    from_py_object
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PyItemId(pub ctx::ItemId);

#[pymethods]
impl PyItemId {
    fn __repr__(&self) -> String {
        format!("ItemId({:?})", self.0.to_string())
    }

    fn __str__(&self) -> String {
        self.0.to_string()
    }
}

/// Where an item came from. Required on every item.
#[pyclass(name = "Provenance", module = "semanticore_context", frozen, eq)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyProvenance(pub ctx::Provenance);

#[pymethods]
impl PyProvenance {
    #[new]
    #[pyo3(signature = (source, derived_from = Vec::new()))]
    fn new(source: String, derived_from: Vec<PyItemId>) -> Self {
        Self(ctx::Provenance::new(source).with_derived_from(derived_from.into_iter().map(|i| i.0)))
    }

    /// Who or what produced the item.
    #[getter]
    fn source(&self) -> &str {
        &self.0.source
    }

    /// Items this one was derived from.
    #[getter]
    fn derived_from(&self) -> Vec<PyItemId> {
        self.0.derived_from.iter().copied().map(PyItemId).collect()
    }

    fn __repr__(&self) -> String {
        let ids: Vec<String> = self.0.derived_from.iter().map(|i| i.to_string()).collect();
        format!("Provenance({:?}, derived_from={:?})", self.0.source, ids)
    }
}

/// `Provenance` or a bare `str` source.
pub fn provenance_from_py(obj: &Bound<'_, PyAny>) -> PyResult<ctx::Provenance> {
    if let Ok(p) = obj.cast::<PyProvenance>() {
        return Ok(p.get().0.clone());
    }
    if let Ok(s) = obj.cast::<PyString>() {
        return Ok(ctx::Provenance::new(s.to_str()?));
    }
    Err(PyTypeError::new_err(
        "provenance must be a Provenance or a str source",
    ))
}

/// The unit of content: a payload (triples, an event or a chunk) plus
/// provenance, an optional scope hint and an optional embedding.
#[pyclass(name = "ContextItem", module = "semanticore_context", frozen, eq)]
#[derive(Debug, Clone, PartialEq)]
pub struct PyContextItem(pub ctx::ContextItem);

fn finish(
    item: ctx::ContextItem,
    scope_hint: Option<PyContextScope>,
    embedding: Option<Vec<f32>>,
) -> PyContextItem {
    let mut item = item;
    item.scope_hint = scope_hint.map(Into::into);
    item.embedding = embedding;
    PyContextItem(item)
}

#[pymethods]
impl PyContextItem {
    /// An item holding RDF triples, each a `(subject, predicate, object)`
    /// tuple.
    #[staticmethod]
    #[pyo3(signature = (triples, provenance, *, scope_hint = None, embedding = None))]
    pub(crate) fn from_triples(
        triples: &Bound<'_, PyAny>,
        provenance: &Bound<'_, PyAny>,
        scope_hint: Option<PyContextScope>,
        embedding: Option<Vec<f32>>,
    ) -> PyResult<Self> {
        let quads = triples
            .try_iter()?
            .map(|t| triple_from_py(&t?))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(finish(
            ctx::ContextItem::triples(quads, provenance_from_py(provenance)?),
            scope_hint,
            embedding,
        ))
    }

    /// A typed structured event with a JSON-compatible body.
    #[staticmethod]
    #[pyo3(signature = (kind, body, provenance, *, scope_hint = None, embedding = None))]
    fn event(
        kind: String,
        body: &Bound<'_, PyAny>,
        provenance: &Bound<'_, PyAny>,
        scope_hint: Option<PyContextScope>,
        embedding: Option<Vec<f32>>,
    ) -> PyResult<Self> {
        Ok(finish(
            ctx::ContextItem::event(kind, json_from_py(body)?, provenance_from_py(provenance)?),
            scope_hint,
            embedding,
        ))
    }

    /// A chunk of text or other content with its media type.
    #[staticmethod]
    #[pyo3(signature = (content, content_type, provenance, *, scope_hint = None, embedding = None))]
    fn chunk(
        content: String,
        content_type: String,
        provenance: &Bound<'_, PyAny>,
        scope_hint: Option<PyContextScope>,
        embedding: Option<Vec<f32>>,
    ) -> PyResult<Self> {
        Ok(finish(
            ctx::ContextItem::chunk(content, content_type, provenance_from_py(provenance)?),
            scope_hint,
            embedding,
        ))
    }

    /// `"triples"`, `"event"` or `"chunk"`.
    #[getter]
    fn payload_type(&self) -> &'static str {
        match &self.0.payload {
            ctx::ContextPayload::Triples(_) => "triples",
            ctx::ContextPayload::Event { .. } => "event",
            ctx::ContextPayload::Chunk { .. } => "chunk",
        }
    }

    /// The triples of a `"triples"` item, else `None`.
    #[getter]
    fn triples<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyList>>> {
        let ctx::ContextPayload::Triples(quads) = &self.0.payload else {
            return Ok(None);
        };
        let list = PyList::empty(py);
        for q in quads {
            list.append(triple_to_py(py, q)?)?;
        }
        Ok(Some(list))
    }

    /// The kind of an `"event"` item, else `None`.
    #[getter]
    fn event_kind(&self) -> Option<&str> {
        match &self.0.payload {
            ctx::ContextPayload::Event { kind, .. } => Some(kind),
            _ => None,
        }
    }

    /// The body of an `"event"` item, else `None`.
    #[getter]
    fn event_body<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        match &self.0.payload {
            ctx::ContextPayload::Event { body, .. } => json_to_py(py, body).map(Some),
            _ => Ok(None),
        }
    }

    /// The content of a `"chunk"` item, else `None`.
    #[getter]
    fn content(&self) -> Option<&str> {
        match &self.0.payload {
            ctx::ContextPayload::Chunk { content, .. } => Some(content),
            _ => None,
        }
    }

    /// The media type of a `"chunk"` item, else `None`.
    #[getter]
    fn content_type(&self) -> Option<&str> {
        match &self.0.payload {
            ctx::ContextPayload::Chunk { content_type, .. } => Some(content_type),
            _ => None,
        }
    }

    #[getter]
    fn provenance(&self) -> PyProvenance {
        PyProvenance(self.0.provenance.clone())
    }

    #[getter]
    fn scope_hint(&self) -> Option<PyContextScope> {
        self.0.scope_hint.map(Into::into)
    }

    #[getter]
    fn embedding(&self) -> Option<Vec<f32>> {
        self.0.embedding.clone()
    }

    /// A copy with a different provenance.
    fn with_provenance(&self, provenance: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut item = self.0.clone();
        item.provenance = provenance_from_py(provenance)?;
        Ok(Self(item))
    }

    fn __repr__(&self) -> String {
        let payload = match &self.0.payload {
            ctx::ContextPayload::Triples(q) => format!("{} triple(s)", q.len()),
            ctx::ContextPayload::Event { kind, .. } => format!("event {kind:?}"),
            ctx::ContextPayload::Chunk { content_type, .. } => format!("chunk {content_type:?}"),
        };
        format!(
            "<ContextItem {payload} from {:?}>",
            self.0.provenance.source
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::ffi::c_str;
    use semanticore_core::model::{NamedNode, Quad};

    #[test]
    fn scopes_convert_both_ways_and_keep_their_order() {
        for s in ctx::ContextScope::ALL {
            assert_eq!(ctx::ContextScope::from(PyContextScope::from(s)), s);
        }
        assert!(PyContextScope::Minor < PyContextScope::Major);
        assert!(PyContextScope::Major < PyContextScope::Enduring);
        assert_eq!(PyContextScope::Major.colder(), Some(PyContextScope::Minor));
        assert_eq!(PyContextScope::Minor.colder(), None);
        assert_eq!(
            PyContextScope::all(),
            vec![
                PyContextScope::Enduring,
                PyContextScope::Major,
                PyContextScope::Minor
            ]
        );
        assert_eq!(PyContextScope::Enduring.__str__(), "enduring");
    }

    #[test]
    fn from_triples_builds_the_rust_item() {
        Python::attach(|py| {
            let triples = py
                .eval(c_str!("[('urn:a', 'urn:p', 'urn:b')]"), None, None)
                .unwrap();
            let prov = PyString::new(py, "chat").into_any();
            let item = PyContextItem::from_triples(
                &triples,
                &prov,
                Some(PyContextScope::Major),
                Some(vec![0.5]),
            )
            .unwrap();
            let expected = ctx::ContextItem::triples(
                vec![Quad::triple(
                    NamedNode::new("urn:a").into(),
                    NamedNode::new("urn:p"),
                    NamedNode::new("urn:b").into(),
                )],
                ctx::Provenance::new("chat"),
            )
            .with_scope_hint(ctx::ContextScope::Major)
            .with_embedding(vec![0.5]);
            assert_eq!(item.0, expected);
            assert_eq!(item.payload_type(), "triples");
            assert!(item.triples(py).unwrap().unwrap().len() == 1);
            assert_eq!(item.event_kind(), None);
        });
    }

    #[test]
    fn event_and_chunk_items_expose_their_payloads() {
        Python::attach(|py| {
            let body = py.eval(c_str!("{'tool': 'search'}"), None, None).unwrap();
            let prov = Bound::new(py, PyProvenance::new("agent".into(), vec![]))
                .unwrap()
                .into_any();
            let e = PyContextItem::event("tool_call".into(), &body, &prov, None, None).unwrap();
            assert_eq!(e.payload_type(), "event");
            assert_eq!(e.event_kind(), Some("tool_call"));
            assert!(e.event_body(py).unwrap().unwrap().eq(&body).unwrap());
            assert!(e.triples(py).unwrap().is_none());

            let c =
                PyContextItem::chunk("hi".into(), "text/plain".into(), &prov, None, None).unwrap();
            assert_eq!(c.payload_type(), "chunk");
            assert_eq!(
                (c.content(), c.content_type()),
                (Some("hi"), Some("text/plain"))
            );
            assert_eq!(c.0.provenance.source, "agent");
        });
    }

    #[test]
    fn provenance_accepts_a_provenance_or_a_str_only() {
        Python::attach(|py| {
            let s = PyString::new(py, "src").into_any();
            assert_eq!(provenance_from_py(&s).unwrap(), ctx::Provenance::new("src"));
            let n = py.eval(c_str!("1"), None, None).unwrap();
            assert!(provenance_from_py(&n)
                .unwrap_err()
                .is_instance_of::<PyTypeError>(py));
        });
    }
}
