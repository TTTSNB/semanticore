//! `Session` and the stats types it returns.
//!
//! The Python `Session` owns the native session behind an `RwLock`: queries
//! and other read-only calls share it, mutations take it exclusively. Every
//! call into the native session runs detached from the interpreter (the GIL
//! is released), so other Python threads keep running meanwhile.

use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use pyo3::exceptions::PyNotImplementedError;
use pyo3::prelude::*;
use pyo3::types::PyType;
use semanticore_context as ctx;
use semanticore_core::datalog::DatalogProgram;

use crate::config::{PyEvictionCriteria, PySessionConfig};
use crate::errors::{closed_err, to_py_err};
use crate::item::{PyContextItem, PyContextScope, PyItemId};
use crate::query::{PyContextQuery, PyContextQueryResult};

/// Counters reported by `Session.close`.
#[pyclass(
    name = "SessionStats",
    module = "semanticore_context",
    frozen,
    eq,
    get_all
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PySessionStats {
    pub items_added: u64,
    pub queries_served: u64,
    pub reasoning_passes: u64,
    pub items_promoted: u64,
    pub items_demoted: u64,
    pub items_evicted: u64,
}

impl From<ctx::SessionStats> for PySessionStats {
    fn from(s: ctx::SessionStats) -> Self {
        Self {
            items_added: s.items_added,
            queries_served: s.queries_served,
            reasoning_passes: s.reasoning_passes,
            items_promoted: s.items_promoted,
            items_demoted: s.items_demoted,
            items_evicted: s.items_evicted,
        }
    }
}

#[pymethods]
impl PySessionStats {
    fn __repr__(&self) -> String {
        format!("{self:?}").replacen("PySessionStats", "SessionStats", 1)
    }
}

/// Returned by `Session.promote`.
#[pyclass(
    name = "PromotionStats",
    module = "semanticore_context",
    frozen,
    eq,
    get_all
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PyPromotionStats {
    pub promoted: u64,
}

#[pymethods]
impl PyPromotionStats {
    fn __repr__(&self) -> String {
        format!("PromotionStats(promoted={})", self.promoted)
    }
}

/// Returned by `Session.reason`. No reasoner ships at v0.1, so this is
/// never returned yet.
#[pyclass(
    name = "ReasoningStats",
    module = "semanticore_context",
    frozen,
    eq,
    get_all
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PyReasoningStats {
    pub quads_added: u64,
}

#[pymethods]
impl PyReasoningStats {
    fn __repr__(&self) -> String {
        format!("ReasoningStats(quads_added={})", self.quads_added)
    }
}

/// A point-in-time copy of one scope.
#[pyclass(name = "ContextSnapshot", module = "semanticore_context", frozen, eq)]
#[derive(Debug, Clone, PartialEq)]
pub struct PyContextSnapshot(pub ctx::ContextSnapshot);

#[pymethods]
impl PyContextSnapshot {
    #[getter]
    fn scope(&self) -> PyContextScope {
        self.0.scope.into()
    }

    /// `(ItemId, ContextItem)` pairs, oldest first.
    #[getter]
    fn items(&self) -> Vec<(PyItemId, PyContextItem)> {
        self.0
            .items
            .iter()
            .map(|(id, item)| (PyItemId(*id), PyContextItem(item.clone())))
            .collect()
    }

    /// Quads in the scope's storage: payload triples plus one metadata
    /// quad per item.
    #[getter]
    fn quad_count(&self) -> u64 {
        self.0.quad_count
    }

    fn __repr__(&self) -> String {
        format!(
            "<ContextSnapshot {} {} item(s)>",
            self.0.scope,
            self.0.items.len()
        )
    }
}

enum State {
    // Boxed: a Session is ~0.5 KiB, the closed state a few counters.
    Open(Box<ctx::Session>),
    Closed(ctx::SessionStats),
}

/// Why a call into the native session failed.
enum CallError {
    Closed,
    Session(ctx::SessionError),
}

impl From<ctx::SessionError> for CallError {
    fn from(e: ctx::SessionError) -> Self {
        CallError::Session(e)
    }
}

fn raise<T>(py: Python<'_>, r: Result<T, CallError>) -> PyResult<T> {
    r.map_err(|e| match e {
        CallError::Closed => closed_err(),
        CallError::Session(e) => to_py_err(py, e),
    })
}

/// A Context OS session over the enduring, major and minor tiers.
///
/// Use it as a context manager so it is always closed:
///
/// ```python
/// with Session.open(SessionConfig.oxigraph(path)) as session:
///     ...
/// ```
#[pyclass(name = "Session", module = "semanticore_context")]
pub struct PySession {
    state: RwLock<State>,
}

impl PySession {
    fn read_state(&self) -> RwLockReadGuard<'_, State> {
        // A panic while holding the lock leaves the native session as
        // consistent as the Rust crate's own Mutex recovery does.
        self.state.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write_state(&self) -> RwLockWriteGuard<'_, State> {
        self.state.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Run `f` on the open native session with the GIL released.
    fn read<T: Send>(
        &self,
        py: Python<'_>,
        f: impl FnOnce(&ctx::Session) -> Result<T, ctx::SessionError> + Send,
    ) -> PyResult<T> {
        let r = py.detach(|| match &*self.read_state() {
            State::Open(s) => f(s).map_err(CallError::from),
            State::Closed(_) => Err(CallError::Closed),
        });
        raise(py, r)
    }

    /// Run `f` on the open native session, exclusively, with the GIL
    /// released.
    fn write<T: Send>(
        &self,
        py: Python<'_>,
        f: impl FnOnce(&mut ctx::Session) -> Result<T, ctx::SessionError> + Send,
    ) -> PyResult<T> {
        let r = py.detach(|| match &mut *self.write_state() {
            State::Open(s) => f(s).map_err(CallError::from),
            State::Closed(_) => Err(CallError::Closed),
        });
        raise(py, r)
    }
}

#[pymethods]
impl PySession {
    /// Open a Session: opens the configured stores and reloads any items
    /// already in the enduring store.
    #[classmethod]
    fn open(
        _cls: &Bound<'_, PyType>,
        py: Python<'_>,
        config: PyRef<'_, PySessionConfig>,
    ) -> PyResult<Self> {
        let native = config.to_native(py).map_err(|e| to_py_err(py, e))?;
        let session = py
            .detach(|| ctx::Session::open(native))
            .map_err(|e| to_py_err(py, e))?;
        Ok(Self {
            state: RwLock::new(State::Open(Box::new(session))),
        })
    }

    /// Close the Session and return its counters. Closing again returns
    /// the same counters.
    fn close(&self, py: Python<'_>) -> PyResult<PySessionStats> {
        let r = py.detach(|| {
            let mut guard = self.write_state();
            let prior = std::mem::replace(&mut *guard, State::Closed(ctx::SessionStats::default()));
            match prior {
                State::Closed(stats) => {
                    *guard = State::Closed(stats);
                    Ok(stats)
                }
                State::Open(session) => {
                    let stats = session.close()?;
                    *guard = State::Closed(stats);
                    Ok(stats)
                }
            }
        });
        raise(py, r).map(Into::into)
    }

    /// Whether `close` has been called.
    #[getter]
    fn closed(&self) -> bool {
        matches!(&*self.read_state(), State::Closed(_))
    }

    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    /// Close the Session; exceptions from the block propagate.
    #[pyo3(signature = (_exc_type = None, _exc = None, _tb = None))]
    fn __exit__(
        &self,
        py: Python<'_>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc: Option<&Bound<'_, PyAny>>,
        _tb: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        self.close(py)?;
        Ok(false)
    }

    /// Add one item to `scope` and return its id.
    fn add(
        &self,
        py: Python<'_>,
        scope: PyContextScope,
        item: PyRef<'_, PyContextItem>,
    ) -> PyResult<PyItemId> {
        let item = item.0.clone();
        self.write(py, |s| s.add(scope.into(), item)).map(PyItemId)
    }

    /// Add items to `scope` in one storage transaction: all or none.
    fn add_batch(
        &self,
        py: Python<'_>,
        scope: PyContextScope,
        items: Vec<PyRef<'_, PyContextItem>>,
    ) -> PyResult<Vec<PyItemId>> {
        let items: Vec<ctx::ContextItem> = items.iter().map(|i| i.0.clone()).collect();
        let ids = self.write(py, |s| s.add_batch(scope.into(), &items))?;
        Ok(ids.into_iter().map(PyItemId).collect())
    }

    /// Remove an item. Returns `False` if it is not in `scope`.
    fn remove(&self, py: Python<'_>, scope: PyContextScope, item_id: PyItemId) -> PyResult<bool> {
        self.write(py, |s| s.remove(scope.into(), &item_id.0))
    }

    /// Run a query. Returned items count as accessed, and auto-promotion
    /// runs afterwards.
    fn query(
        &self,
        py: Python<'_>,
        query: PyRef<'_, PyContextQuery>,
    ) -> PyResult<PyContextQueryResult> {
        let q = query.0.clone();
        self.read(py, |s| s.query(q)).map(PyContextQueryResult)
    }

    /// Materialize entailments for `scope`. Not yet implemented: raises
    /// `NotYetImplementedError`.
    fn reason(&self, py: Python<'_>, scope: PyContextScope) -> PyResult<PyReasoningStats> {
        self.write(py, |s| s.reason(scope.into()))
            .map(|r| PyReasoningStats {
                quads_added: r.quads_added,
            })
    }

    /// SHACL validation of `data` against shapes in `shapes`. Premium-only
    /// at v0.1: raises `ShaclUnavailableError`.
    fn validate(
        &self,
        py: Python<'_>,
        data: PyContextScope,
        shapes: PyContextScope,
    ) -> PyResult<()> {
        self.read(py, |s| s.validate(data.into(), shapes.into()))?;
        Err(PyNotImplementedError::new_err(
            "SHACL validation reports are not marshaled to Python yet",
        ))
    }

    /// Datalog evaluation over `scope`. Premium-only at v0.1: raises
    /// `DatalogUnavailableError`.
    fn evaluate_datalog(&self, py: Python<'_>, scope: PyContextScope) -> PyResult<()> {
        self.read(py, |s| {
            s.evaluate_datalog(&DatalogProgram::default(), scope.into())
        })?;
        Err(PyNotImplementedError::new_err(
            "Datalog programs and results are not marshaled to Python yet",
        ))
    }

    /// Move items from `from_scope` to the equal-or-more-durable
    /// `to_scope`. Every item must be in `from_scope`; otherwise nothing
    /// moves.
    fn promote(
        &self,
        py: Python<'_>,
        item_ids: Vec<PyItemId>,
        from_scope: PyContextScope,
        to_scope: PyContextScope,
    ) -> PyResult<PyPromotionStats> {
        let ids: Vec<ctx::ItemId> = item_ids.into_iter().map(|i| i.0).collect();
        self.write(py, |s| s.promote(&ids, from_scope.into(), to_scope.into()))
            .map(|p| PyPromotionStats {
                promoted: p.promoted,
            })
    }

    /// Evict items from `scope`; returns how many left it, dropped or
    /// demoted.
    fn evict(
        &self,
        py: Python<'_>,
        scope: PyContextScope,
        criteria: PyRef<'_, PyEvictionCriteria>,
    ) -> PyResult<u64> {
        let c = criteria.0.clone();
        self.write(py, |s| s.evict(scope.into(), c))
    }

    /// A copy of `scope`'s items and its storage's quad count.
    fn snapshot(&self, py: Python<'_>, scope: PyContextScope) -> PyResult<PyContextSnapshot> {
        self.read(py, |s| s.snapshot(scope.into()))
            .map(PyContextSnapshot)
    }

    /// The scope `item_id` is in, or `None` if no live item has that id.
    fn scope_of(&self, py: Python<'_>, item_id: PyItemId) -> PyResult<Option<PyContextScope>> {
        self.read(py, |s| Ok(s.scope_of(item_id.0).map(Into::into)))
    }

    /// How many items are in `scope`.
    fn item_count(&self, py: Python<'_>, scope: PyContextScope) -> PyResult<usize> {
        self.read(py, |s| Ok(s.item_count(scope.into())))
    }

    fn __repr__(&self) -> String {
        if self.closed() {
            "<Session closed>".into()
        } else {
            "<Session open>".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::{
        DatalogUnavailableError, NotYetImplementedError, SessionClosedError, ShaclUnavailableError,
    };
    use pyo3::ffi::c_str;

    fn open(py: Python<'_>, dir: &std::path::Path) -> PySession {
        let cfg = Bound::new(
            py,
            PySessionConfig::oxigraph(py, dir.to_owned(), None, None, None, None).unwrap(),
        )
        .unwrap();
        PySession::open(&py.get_type::<PySession>(), py, cfg.borrow()).unwrap()
    }

    fn item(py: Python<'_>) -> Bound<'_, PyContextItem> {
        let triples = py
            .eval(c_str!("[('urn:a', 'urn:p', 'urn:b')]"), None, None)
            .unwrap();
        let prov = pyo3::types::PyString::new(py, "test").into_any();
        let i = PyContextItem::from_triples(&triples, &prov, None, None).unwrap();
        Bound::new(py, i).unwrap()
    }

    #[test]
    fn calls_after_close_raise_session_closed_and_close_is_idempotent() {
        Python::attach(|py| {
            let dir = tempfile::tempdir().unwrap();
            let s = open(py, dir.path());
            s.add(py, PyContextScope::Major, item(py).borrow()).unwrap();
            let first = s.close(py).unwrap();
            assert_eq!(first.items_added, 1);
            assert!(s.closed());
            assert_eq!(s.close(py).unwrap(), first);
            let err = s
                .add(py, PyContextScope::Major, item(py).borrow())
                .unwrap_err();
            assert!(err.is_instance_of::<SessionClosedError>(py));
            assert!(s
                .item_count(py, PyContextScope::Major)
                .unwrap_err()
                .is_instance_of::<SessionClosedError>(py));
        });
    }

    #[test]
    fn reserved_surfaces_raise_their_typed_errors() {
        Python::attach(|py| {
            let dir = tempfile::tempdir().unwrap();
            let s = open(py, dir.path());
            let e = s
                .validate(py, PyContextScope::Enduring, PyContextScope::Major)
                .unwrap_err();
            assert!(e.is_instance_of::<ShaclUnavailableError>(py));
            let e = s
                .evaluate_datalog(py, PyContextScope::Enduring)
                .unwrap_err();
            assert!(e.is_instance_of::<DatalogUnavailableError>(py));
            let e = s.reason(py, PyContextScope::Minor).unwrap_err();
            assert!(e.is_instance_of::<NotYetImplementedError>(py));
        });
    }

    #[test]
    fn exit_closes_the_session() {
        Python::attach(|py| {
            let dir = tempfile::tempdir().unwrap();
            let s = open(py, dir.path());
            assert!(!s.__exit__(py, None, None, None).unwrap());
            assert!(s.closed());
        });
    }
}
