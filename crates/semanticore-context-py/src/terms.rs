//! RDF terms and JSON values <-> Python objects.
//!
//! IRIs are plain `str`; blank nodes and literals are the [`PyBlankNode`]
//! and [`PyLiteral`] classes. A triple is a 3-tuple
//! `(subject, predicate, object)`:
//!
//! | Position | Python |
//! |---|---|
//! | subject | `str` (IRI) or `BlankNode` |
//! | predicate | `str` (IRI) |
//! | object | `str` (IRI), `BlankNode` or `Literal` |
//!
//! Event bodies are JSON values: `None`, `bool`, `int`, `float`, `str`,
//! `list`/`tuple` and `dict` with `str` keys.

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use semanticore_core::model::{BlankNode, Literal, NamedNode, NamedOrBlankNode, Quad, Term};

/// An RDF blank node.
#[pyclass(name = "BlankNode", module = "semanticore_context", frozen, eq, hash)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PyBlankNode(pub BlankNode);

#[pymethods]
impl PyBlankNode {
    #[new]
    fn new(id: String) -> Self {
        Self(BlankNode::new(id))
    }

    /// The blank-node label.
    #[getter]
    fn id(&self) -> &str {
        self.0.as_str()
    }

    fn __repr__(&self) -> String {
        format!("BlankNode({:?})", self.0.as_str())
    }

    fn __str__(&self) -> String {
        self.0.to_string()
    }
}

/// An RDF literal: a lexical form with a datatype IRI or a language tag.
#[pyclass(name = "Literal", module = "semanticore_context", frozen, eq, hash)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PyLiteral(pub Literal);

#[pymethods]
impl PyLiteral {
    /// `Literal(value)` is an `xsd:string`; pass `datatype` (an IRI) or
    /// `language` (a tag), not both.
    #[new]
    #[pyo3(signature = (value, datatype = None, language = None))]
    fn new(value: String, datatype: Option<String>, language: Option<String>) -> PyResult<Self> {
        Ok(Self(match (datatype, language) {
            (None, None) => Literal::new_simple(value),
            (Some(dt), None) => Literal::new_typed(value, NamedNode::new(dt)),
            (None, Some(lang)) => Literal::new_lang(value, lang),
            (Some(_), Some(_)) => {
                return Err(PyValueError::new_err(
                    "a literal has a datatype or a language tag, not both",
                ))
            }
        }))
    }

    /// The lexical form.
    #[getter]
    fn value(&self) -> &str {
        self.0.value()
    }

    /// The datatype IRI.
    #[getter]
    fn datatype(&self) -> &str {
        self.0.datatype().as_str()
    }

    /// The language tag, if any.
    #[getter]
    fn language(&self) -> Option<&str> {
        self.0.language()
    }

    fn __repr__(&self) -> String {
        match self.0.language() {
            Some(lang) => format!("Literal({:?}, language={lang:?})", self.0.value()),
            None => format!(
                "Literal({:?}, datatype={:?})",
                self.0.value(),
                self.0.datatype().as_str()
            ),
        }
    }

    fn __str__(&self) -> String {
        self.0.to_string()
    }
}

fn type_name(obj: &Bound<'_, PyAny>) -> String {
    obj.get_type()
        .name()
        .map(|n| n.to_string())
        .unwrap_or_else(|_| "?".into())
}

/// A subject: `str` (IRI) or `BlankNode`.
pub fn subject_from_py(obj: &Bound<'_, PyAny>) -> PyResult<NamedOrBlankNode> {
    if let Ok(s) = obj.cast::<PyString>() {
        return Ok(NamedOrBlankNode::Named(NamedNode::new(s.to_str()?)));
    }
    if let Ok(b) = obj.cast::<PyBlankNode>() {
        return Ok(NamedOrBlankNode::Blank(b.get().0.clone()));
    }
    Err(PyTypeError::new_err(format!(
        "subject must be an IRI str or a BlankNode, not {}",
        type_name(obj)
    )))
}

/// A predicate: `str` (IRI).
pub fn predicate_from_py(obj: &Bound<'_, PyAny>) -> PyResult<NamedNode> {
    match obj.cast::<PyString>() {
        Ok(s) => Ok(NamedNode::new(s.to_str()?)),
        Err(_) => Err(PyTypeError::new_err(format!(
            "predicate must be an IRI str, not {}",
            type_name(obj)
        ))),
    }
}

/// An object: `str` (IRI), `BlankNode` or `Literal`.
pub fn object_from_py(obj: &Bound<'_, PyAny>) -> PyResult<Term> {
    if let Ok(s) = obj.cast::<PyString>() {
        return Ok(Term::Named(NamedNode::new(s.to_str()?)));
    }
    if let Ok(b) = obj.cast::<PyBlankNode>() {
        return Ok(Term::Blank(b.get().0.clone()));
    }
    if let Ok(l) = obj.cast::<PyLiteral>() {
        return Ok(Term::Literal(l.get().0.clone()));
    }
    Err(PyTypeError::new_err(format!(
        "object must be an IRI str, a BlankNode or a Literal, not {}",
        type_name(obj)
    )))
}

pub fn subject_to_py<'py>(py: Python<'py>, s: &NamedOrBlankNode) -> PyResult<Bound<'py, PyAny>> {
    Ok(match s {
        NamedOrBlankNode::Named(n) => PyString::new(py, n.as_str()).into_any(),
        NamedOrBlankNode::Blank(b) => Bound::new(py, PyBlankNode(b.clone()))?.into_any(),
    })
}

pub fn object_to_py<'py>(py: Python<'py>, t: &Term) -> PyResult<Bound<'py, PyAny>> {
    Ok(match t {
        Term::Named(n) => PyString::new(py, n.as_str()).into_any(),
        Term::Blank(b) => Bound::new(py, PyBlankNode(b.clone()))?.into_any(),
        Term::Literal(l) => Bound::new(py, PyLiteral(l.clone()))?.into_any(),
    })
}

/// A `(subject, predicate, object)` tuple to a default-graph triple.
pub fn triple_from_py(obj: &Bound<'_, PyAny>) -> PyResult<Quad> {
    let t = obj.cast::<PyTuple>().map_err(|_| {
        PyTypeError::new_err(format!(
            "a triple is a (subject, predicate, object) tuple, not {}",
            type_name(obj)
        ))
    })?;
    if t.len() != 3 {
        return Err(PyValueError::new_err(format!(
            "a triple has 3 terms, not {}",
            t.len()
        )));
    }
    Ok(Quad::triple(
        subject_from_py(&t.get_item(0)?)?,
        predicate_from_py(&t.get_item(1)?)?,
        object_from_py(&t.get_item(2)?)?,
    ))
}

/// A triple to `(subject, predicate, object)`. The graph is not exposed:
/// item triples are always in the default graph.
pub fn triple_to_py<'py>(py: Python<'py>, q: &Quad) -> PyResult<Bound<'py, PyTuple>> {
    PyTuple::new(
        py,
        [
            subject_to_py(py, &q.subject)?,
            PyString::new(py, q.predicate.as_str()).into_any(),
            object_to_py(py, &q.object)?,
        ],
    )
}

/// A Python value to JSON. `bool` is checked before `int` (it is a
/// subclass); non-finite floats and ints outside the i64/u64 range are
/// refused because JSON cannot carry them.
pub fn json_from_py(obj: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    use serde_json::Value;
    if obj.is_none() {
        return Ok(Value::Null);
    }
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(Value::Bool(b.is_true()));
    }
    if obj.cast::<PyInt>().is_ok() {
        if let Ok(i) = obj.extract::<i64>() {
            return Ok(Value::from(i));
        }
        if let Ok(u) = obj.extract::<u64>() {
            return Ok(Value::from(u));
        }
        return Err(PyValueError::new_err(
            "integer out of range for a JSON event body (i64/u64)",
        ));
    }
    if let Ok(f) = obj.cast::<PyFloat>() {
        return serde_json::Number::from_f64(f.value())
            .map(Value::Number)
            .ok_or_else(|| PyValueError::new_err("event bodies cannot hold NaN or infinity"));
    }
    if let Ok(s) = obj.cast::<PyString>() {
        return Ok(Value::String(s.to_str()?.to_owned()));
    }
    if let Ok(l) = obj.cast::<PyList>() {
        return l.iter().map(|v| json_from_py(&v)).collect();
    }
    if let Ok(t) = obj.cast::<PyTuple>() {
        return t.iter().map(|v| json_from_py(&v)).collect();
    }
    if let Ok(d) = obj.cast::<PyDict>() {
        let mut map = serde_json::Map::with_capacity(d.len());
        for (k, v) in d.iter() {
            let k = k.cast::<PyString>().map_err(|_| {
                PyTypeError::new_err(format!(
                    "event body keys must be str, not {}",
                    type_name(&k)
                ))
            })?;
            map.insert(k.to_str()?.to_owned(), json_from_py(&v)?);
        }
        return Ok(Value::Object(map));
    }
    Err(PyTypeError::new_err(format!(
        "{} is not JSON-serializable",
        type_name(obj)
    )))
}

/// JSON to the matching Python value (`dict`, `list`, `str`, `int`,
/// `float`, `bool` or `None`).
pub fn json_to_py<'py>(py: Python<'py>, v: &serde_json::Value) -> PyResult<Bound<'py, PyAny>> {
    use serde_json::Value;
    Ok(match v {
        Value::Null => py.None().into_bound(py),
        Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into_pyobject(py)?.into_any()
            } else if let Some(u) = n.as_u64() {
                u.into_pyobject(py)?.into_any()
            } else {
                // Without serde_json's arbitrary_precision every Number is
                // an i64, a u64 or an f64.
                PyFloat::new(py, n.as_f64().unwrap_or(f64::NAN)).into_any()
            }
        }
        Value::String(s) => PyString::new(py, s).into_any(),
        Value::Array(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(json_to_py(py, item)?)?;
            }
            list.into_any()
        }
        Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, json_to_py(py, v)?)?;
            }
            dict.into_any()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::ffi::c_str;

    fn eval<'py>(py: Python<'py>, code: &std::ffi::CStr) -> Bound<'py, PyAny> {
        let m = PyModule::new(py, "t").unwrap();
        m.add_class::<PyBlankNode>().unwrap();
        m.add_class::<PyLiteral>().unwrap();
        py.eval(code, Some(&m.dict()), None).unwrap()
    }

    #[test]
    fn str_terms_are_iris_in_every_position() {
        Python::attach(|py| {
            let t = eval(py, c_str!("('urn:s', 'urn:p', 'urn:o')"));
            let q = triple_from_py(&t).unwrap();
            assert_eq!(
                q,
                Quad::triple(
                    NamedNode::new("urn:s").into(),
                    NamedNode::new("urn:p"),
                    NamedNode::new("urn:o").into()
                )
            );
        });
    }

    #[test]
    fn blank_nodes_and_literals_convert_both_ways() {
        Python::attach(|py| {
            let t = eval(
                py,
                c_str!("(BlankNode('b0'), 'urn:p', Literal('chat', language='en'))"),
            );
            let q = triple_from_py(&t).unwrap();
            assert_eq!(q.subject, NamedOrBlankNode::Blank(BlankNode::new("b0")));
            assert_eq!(q.object, Term::Literal(Literal::new_lang("chat", "en")));
            let back = triple_to_py(py, &q).unwrap();
            assert!(back.eq(&t).unwrap(), "round trip");
        });
    }

    #[test]
    fn typed_and_simple_literals() {
        Python::attach(|py| {
            let typed = eval(
                py,
                c_str!("Literal('3', datatype='http://www.w3.org/2001/XMLSchema#integer')"),
            );
            assert_eq!(
                object_from_py(&typed).unwrap(),
                Term::Literal(Literal::new_typed(
                    "3",
                    NamedNode::new("http://www.w3.org/2001/XMLSchema#integer")
                ))
            );
            let simple = eval(py, c_str!("Literal('x')"));
            assert_eq!(
                object_from_py(&simple).unwrap(),
                Term::Literal(Literal::new_simple("x"))
            );
        });
    }

    #[test]
    fn literal_refuses_datatype_and_language_together() {
        assert!(PyLiteral::new("x".into(), Some("urn:dt".into()), Some("en".into())).is_err());
    }

    #[test]
    fn positions_refuse_the_wrong_term_kinds() {
        Python::attach(|py| {
            let lit = eval(py, c_str!("Literal('x')"));
            let blank = eval(py, c_str!("BlankNode('b')"));
            let n = eval(py, c_str!("3"));
            assert!(subject_from_py(&lit)
                .unwrap_err()
                .is_instance_of::<PyTypeError>(py));
            assert!(predicate_from_py(&blank)
                .unwrap_err()
                .is_instance_of::<PyTypeError>(py));
            assert!(object_from_py(&n)
                .unwrap_err()
                .is_instance_of::<PyTypeError>(py));
        });
    }

    #[test]
    fn triples_must_be_three_tuples() {
        Python::attach(|py| {
            let short = eval(py, c_str!("('urn:s', 'urn:p')"));
            assert!(triple_from_py(&short)
                .unwrap_err()
                .is_instance_of::<PyValueError>(py));
            let list = eval(py, c_str!("['urn:s', 'urn:p', 'urn:o']"));
            assert!(triple_from_py(&list)
                .unwrap_err()
                .is_instance_of::<PyTypeError>(py));
        });
    }

    #[test]
    fn json_round_trips_through_python() {
        Python::attach(|py| {
            let v = serde_json::json!({
                "tool": "search",
                "n": 3,
                "big": u64::MAX,
                "neg": -7,
                "ratio": 0.5,
                "ok": true,
                "none": null,
                "tags": ["a", 1, false],
            });
            let obj = json_to_py(py, &v).unwrap();
            assert!(obj.cast::<PyDict>().is_ok());
            assert_eq!(json_from_py(&obj).unwrap(), v);
        });
    }

    #[test]
    fn json_keeps_bools_distinct_from_ints_and_accepts_tuples() {
        Python::attach(|py| {
            let obj = eval(py, c_str!("{'b': True, 'i': 1, 't': (1, 2)}"));
            assert_eq!(
                json_from_py(&obj).unwrap(),
                serde_json::json!({"b": true, "i": 1, "t": [1, 2]})
            );
        });
    }

    #[test]
    fn json_refuses_what_json_cannot_hold() {
        Python::attach(|py| {
            for code in [
                c_str!("float('nan')"),
                c_str!("2 ** 70"),
                c_str!("-(2 ** 70)"),
            ] {
                let err = json_from_py(&eval(py, code)).unwrap_err();
                assert!(err.is_instance_of::<PyValueError>(py), "{code:?}");
            }
            for code in [c_str!("{1: 'x'}"), c_str!("object()"), c_str!("{1, 2}")] {
                let err = json_from_py(&eval(py, code)).unwrap_err();
                assert!(err.is_instance_of::<PyTypeError>(py), "{code:?}");
            }
        });
    }
}
