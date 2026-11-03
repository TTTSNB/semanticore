//! `SessionConfig` and its parts, plus `EvictionCriteria`.
//!
//! The Rust `SessionConfig` owns open stores, so it cannot be shared or
//! reused. The Python `SessionConfig` instead holds the enduring-store path
//! and the settings; [`PySessionConfig::to_native`] opens the stores and
//! builds the Rust config when `Session.open` is called.

use std::path::PathBuf;
use std::time::Duration;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyFloat, PyInt};
use semanticore_context as ctx;

use crate::item::{PyContextScope, PyItemId};

/// Seconds (`int` or `float`) or a `datetime.timedelta` to a `Duration`.
pub fn duration_from_py(obj: &Bound<'_, PyAny>) -> PyResult<Duration> {
    let secs: f64 = if obj.cast::<PyFloat>().is_ok() || obj.cast::<PyInt>().is_ok() {
        obj.extract()?
    } else if obj.hasattr("total_seconds")? {
        // datetime.timedelta, looked up by method so abi3 builds work.
        obj.call_method0("total_seconds")?.extract()?
    } else {
        return Err(PyTypeError::new_err(
            "a duration is a number of seconds or a datetime.timedelta",
        ));
    };
    Duration::try_from_secs_f64(secs)
        .map_err(|e| PyValueError::new_err(format!("invalid duration {secs} s: {e}")))
}

/// Per-scope item bounds; `None` is unbounded.
#[pyclass(
    name = "ScopeBounds",
    module = "semanticore_context",
    get_all,
    set_all,
    eq
)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PyScopeBounds {
    pub enduring_max_items: Option<u64>,
    pub major_max_items: Option<u64>,
    pub minor_max_items: Option<u64>,
}

#[pymethods]
impl PyScopeBounds {
    #[new]
    #[pyo3(signature = (*, enduring_max_items = None, major_max_items = None, minor_max_items = None))]
    fn new(
        enduring_max_items: Option<u64>,
        major_max_items: Option<u64>,
        minor_max_items: Option<u64>,
    ) -> Self {
        Self {
            enduring_max_items,
            major_max_items,
            minor_max_items,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "ScopeBounds(enduring_max_items={:?}, major_max_items={:?}, minor_max_items={:?})",
            self.enduring_max_items, self.major_max_items, self.minor_max_items
        )
    }
}

impl From<PyScopeBounds> for ctx::ScopeBounds {
    fn from(b: PyScopeBounds) -> Self {
        Self {
            enduring_max_items: b.enduring_max_items,
            major_max_items: b.major_max_items,
            minor_max_items: b.minor_max_items,
        }
    }
}

/// When auto-promotion runs.
#[pyclass(
    name = "PromotionCadence",
    module = "semanticore_context",
    frozen,
    eq,
    hash,
    from_py_object
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PyPromotionCadence {
    /// At the end of every successful `Session.query`.
    #[pyo3(name = "AFTER_EACH_QUERY")]
    AfterEachQuery,
    /// Never; only explicit `Session.promote` moves items up.
    #[pyo3(name = "NEVER")]
    Never,
}

impl From<ctx::PromotionCadence> for PyPromotionCadence {
    fn from(c: ctx::PromotionCadence) -> Self {
        match c {
            ctx::PromotionCadence::AfterEachQuery => Self::AfterEachQuery,
            ctx::PromotionCadence::Never => Self::Never,
        }
    }
}

impl From<PyPromotionCadence> for ctx::PromotionCadence {
    fn from(c: PyPromotionCadence) -> Self {
        match c {
            PyPromotionCadence::AfterEachQuery => Self::AfterEachQuery,
            PyPromotionCadence::Never => Self::Never,
        }
    }
}

/// Auto-promotion thresholds. Defaults come from the Rust crate (RFC 0004:
/// more than 3 accesses within 60 s, more than 5 references).
#[pyclass(name = "PromotionConfig", module = "semanticore_context", eq)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PyPromotionConfig(pub ctx::PromotionConfig);

#[pymethods]
impl PyPromotionConfig {
    #[new]
    #[pyo3(signature = (*, cadence = None, minor_to_major_accesses = None, minor_window = None, major_to_enduring_references = None))]
    fn new(
        cadence: Option<PyPromotionCadence>,
        minor_to_major_accesses: Option<u32>,
        minor_window: Option<&Bound<'_, PyAny>>,
        major_to_enduring_references: Option<u32>,
    ) -> PyResult<Self> {
        let mut c = ctx::PromotionConfig::default();
        if let Some(v) = cadence {
            c.cadence = v.into();
        }
        if let Some(v) = minor_to_major_accesses {
            c.minor_to_major_accesses = v;
        }
        if let Some(v) = minor_window {
            c.minor_window = duration_from_py(v)?;
        }
        if let Some(v) = major_to_enduring_references {
            c.major_to_enduring_references = v;
        }
        Ok(Self(c))
    }

    #[getter]
    fn cadence(&self) -> PyPromotionCadence {
        self.0.cadence.into()
    }

    #[setter]
    fn set_cadence(&mut self, v: PyPromotionCadence) {
        self.0.cadence = v.into();
    }

    /// A minor item returned by more than this many queries within
    /// `minor_window` moves to major.
    #[getter]
    fn minor_to_major_accesses(&self) -> u32 {
        self.0.minor_to_major_accesses
    }

    #[setter]
    fn set_minor_to_major_accesses(&mut self, v: u32) {
        self.0.minor_to_major_accesses = v;
    }

    /// The access window, in seconds. Accepts seconds or a `timedelta`.
    #[getter]
    fn minor_window(&self) -> f64 {
        self.0.minor_window.as_secs_f64()
    }

    #[setter]
    fn set_minor_window(&mut self, v: &Bound<'_, PyAny>) -> PyResult<()> {
        self.0.minor_window = duration_from_py(v)?;
        Ok(())
    }

    /// A major item cited by more than this many other live items moves
    /// to enduring.
    #[getter]
    fn major_to_enduring_references(&self) -> u32 {
        self.0.major_to_enduring_references
    }

    #[setter]
    fn set_major_to_enduring_references(&mut self, v: u32) {
        self.0.major_to_enduring_references = v;
    }

    fn __repr__(&self) -> String {
        format!(
            "PromotionConfig(cadence={:?}, minor_to_major_accesses={}, minor_window={}, major_to_enduring_references={})",
            self.0.cadence,
            self.0.minor_to_major_accesses,
            self.0.minor_window.as_secs_f64(),
            self.0.major_to_enduring_references
        )
    }
}

/// How eviction picks victims within a scope. Build one with `lru()`,
/// `lfu()` or `recency_weighted(half_life)`.
#[pyclass(
    name = "EvictionPolicy",
    module = "semanticore_context",
    frozen,
    eq,
    from_py_object
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PyEvictionPolicy(pub ctx::EvictionPolicy);

#[pymethods]
impl PyEvictionPolicy {
    /// Least recently touched first (the default).
    #[staticmethod]
    fn lru() -> Self {
        Self(ctx::EvictionPolicy::Lru)
    }

    /// Fewest query accesses first.
    #[staticmethod]
    fn lfu() -> Self {
        Self(ctx::EvictionPolicy::Lfu)
    }

    /// Lowest decayed touch weight first; weight halves every
    /// `half_life` (seconds or a `timedelta`).
    #[staticmethod]
    fn recency_weighted(half_life: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(ctx::EvictionPolicy::RecencyWeighted {
            half_life: duration_from_py(half_life)?,
        }))
    }

    /// `"lru"`, `"lfu"` or `"recency_weighted"`.
    #[getter]
    fn kind(&self) -> &'static str {
        match self.0 {
            ctx::EvictionPolicy::Lru => "lru",
            ctx::EvictionPolicy::Lfu => "lfu",
            ctx::EvictionPolicy::RecencyWeighted { .. } => "recency_weighted",
        }
    }

    /// The half-life in seconds, for `recency_weighted`; else `None`.
    #[getter]
    fn half_life(&self) -> Option<f64> {
        match self.0 {
            ctx::EvictionPolicy::RecencyWeighted { half_life } => Some(half_life.as_secs_f64()),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        match self.half_life() {
            Some(h) => format!("EvictionPolicy.recency_weighted({h})"),
            None => format!("EvictionPolicy.{}()", self.kind()),
        }
    }
}

/// What happens to an item that policy eviction selects.
#[pyclass(
    name = "DemotionTarget",
    module = "semanticore_context",
    frozen,
    eq,
    hash,
    from_py_object
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PyDemotionTarget {
    /// Drop it.
    #[pyo3(name = "DROP")]
    Drop,
    /// Move it one scope colder; drop it if it is already minor.
    #[pyo3(name = "NEXT_COLDER")]
    NextColder,
    /// Demote items that arrived by promotion once, drop the rest.
    #[pyo3(name = "ONE_STEP_THEN_DROP")]
    OneStepThenDrop,
}

impl From<ctx::DemotionTarget> for PyDemotionTarget {
    fn from(t: ctx::DemotionTarget) -> Self {
        match t {
            ctx::DemotionTarget::Drop => Self::Drop,
            ctx::DemotionTarget::NextColder => Self::NextColder,
            ctx::DemotionTarget::OneStepThenDrop => Self::OneStepThenDrop,
        }
    }
}

impl From<PyDemotionTarget> for ctx::DemotionTarget {
    fn from(t: PyDemotionTarget) -> Self {
        match t {
            PyDemotionTarget::Drop => Self::Drop,
            PyDemotionTarget::NextColder => Self::NextColder,
            PyDemotionTarget::OneStepThenDrop => Self::OneStepThenDrop,
        }
    }
}

/// How to open a Session: the OSS default storage (a file-backed enduring
/// store at `enduring_path`, in-memory major and minor stores) plus
/// bounds, eviction and promotion settings.
///
/// `bounds` and `promotion` are live objects: `cfg.bounds.major_max_items =
/// 2` changes this config.
#[pyclass(name = "SessionConfig", module = "semanticore_context")]
pub struct PySessionConfig {
    enduring_path: PathBuf,
    #[pyo3(get, set)]
    bounds: Py<PyScopeBounds>,
    #[pyo3(get, set)]
    eviction_policy: PyEvictionPolicy,
    #[pyo3(get, set)]
    demotion_target: PyDemotionTarget,
    #[pyo3(get, set)]
    promotion: Py<PyPromotionConfig>,
}

impl PySessionConfig {
    /// Open the stores and build the Rust config.
    pub fn to_native(&self, py: Python<'_>) -> Result<ctx::SessionConfig, ctx::SessionError> {
        let mut cfg = ctx::SessionConfig::oxigraph(&self.enduring_path)?;
        cfg.bounds = (*self.bounds.borrow(py)).into();
        cfg.eviction_policy = self.eviction_policy.0;
        cfg.demotion_target = self.demotion_target.into();
        cfg.promotion = self.promotion.borrow(py).0;
        Ok(cfg)
    }
}

#[pymethods]
impl PySessionConfig {
    /// The OSS default from RFC 0004: a file-backed Oxigraph store at
    /// `enduring_path` for the enduring tier, in-memory Oxigraph stores for
    /// major and minor. Stores are opened by `Session.open`.
    #[staticmethod]
    #[pyo3(signature = (enduring_path, *, bounds = None, eviction_policy = None, demotion_target = None, promotion = None))]
    pub(crate) fn oxigraph(
        py: Python<'_>,
        enduring_path: PathBuf,
        bounds: Option<Py<PyScopeBounds>>,
        eviction_policy: Option<PyEvictionPolicy>,
        demotion_target: Option<PyDemotionTarget>,
        promotion: Option<Py<PyPromotionConfig>>,
    ) -> PyResult<Self> {
        let bounds = match bounds {
            Some(b) => b,
            None => Py::new(py, PyScopeBounds::default())?,
        };
        let promotion = match promotion {
            Some(p) => p,
            None => Py::new(py, PyPromotionConfig(ctx::PromotionConfig::default()))?,
        };
        Ok(Self {
            enduring_path,
            bounds,
            eviction_policy: eviction_policy
                .unwrap_or(PyEvictionPolicy(ctx::EvictionPolicy::default())),
            demotion_target: demotion_target.unwrap_or(ctx::DemotionTarget::default().into()),
            promotion,
        })
    }

    /// Where the enduring tier lives on disk.
    #[getter]
    fn enduring_path(&self) -> PathBuf {
        self.enduring_path.clone()
    }

    fn __repr__(&self) -> String {
        format!("<SessionConfig oxigraph {:?}>", self.enduring_path)
    }
}

/// Which items `Session.evict` removes. Build one with `items(ids)`,
/// `demote(ids, to)` or `policy(count)`.
#[pyclass(name = "EvictionCriteria", module = "semanticore_context", frozen, eq)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyEvictionCriteria(pub ctx::EvictionCriteria);

fn ids(v: Vec<PyItemId>) -> Vec<ctx::ItemId> {
    v.into_iter().map(|i| i.0).collect()
}

#[pymethods]
impl PyEvictionCriteria {
    /// Drop these items.
    #[staticmethod]
    fn items(item_ids: Vec<PyItemId>) -> Self {
        Self(ctx::EvictionCriteria::Items(ids(item_ids)))
    }

    /// Move these items to the strictly colder scope `to`.
    #[staticmethod]
    fn demote(item_ids: Vec<PyItemId>, to: PyContextScope) -> Self {
        Self(ctx::EvictionCriteria::Demote {
            items: ids(item_ids),
            to: to.into(),
        })
    }

    /// Let the eviction policy pick `count` items and apply the demotion
    /// target to each.
    #[staticmethod]
    fn policy(count: u64) -> Self {
        Self(ctx::EvictionCriteria::Policy { count })
    }

    fn __repr__(&self) -> String {
        format!("<EvictionCriteria {:?}>", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::ffi::c_str;

    #[test]
    fn durations_accept_seconds_and_timedeltas() {
        Python::attach(|py| {
            let ev = |c| py.eval(c, None, None).unwrap();
            assert_eq!(
                duration_from_py(&ev(c_str!("2"))).unwrap(),
                Duration::from_secs(2)
            );
            assert_eq!(
                duration_from_py(&ev(c_str!("0.25"))).unwrap(),
                Duration::from_millis(250)
            );
            assert_eq!(
                duration_from_py(&ev(c_str!(
                    "__import__('datetime').timedelta(milliseconds=1500)"
                )))
                .unwrap(),
                Duration::from_millis(1500)
            );
            assert!(duration_from_py(&ev(c_str!("-1")))
                .unwrap_err()
                .is_instance_of::<PyValueError>(py));
            assert!(duration_from_py(&ev(c_str!("'1s'")))
                .unwrap_err()
                .is_instance_of::<PyTypeError>(py));
        });
    }

    #[test]
    fn promotion_defaults_come_from_the_rust_crate() {
        let c = PyPromotionConfig::new(None, None, None, None).unwrap();
        assert_eq!(c.0, ctx::PromotionConfig::default());
        assert_eq!(c.minor_to_major_accesses(), 3);
        assert_eq!(c.minor_window(), 60.0);
        assert_eq!(c.major_to_enduring_references(), 5);
        assert_eq!(c.cadence(), PyPromotionCadence::AfterEachQuery);
    }

    #[test]
    fn enums_convert_both_ways() {
        for t in [
            ctx::DemotionTarget::Drop,
            ctx::DemotionTarget::NextColder,
            ctx::DemotionTarget::OneStepThenDrop,
        ] {
            assert_eq!(ctx::DemotionTarget::from(PyDemotionTarget::from(t)), t);
        }
        for c in [
            ctx::PromotionCadence::AfterEachQuery,
            ctx::PromotionCadence::Never,
        ] {
            assert_eq!(ctx::PromotionCadence::from(PyPromotionCadence::from(c)), c);
        }
        assert_eq!(PyEvictionPolicy::lfu().kind(), "lfu");
        assert_eq!(PyEvictionPolicy::lru().half_life(), None);
    }

    #[test]
    fn to_native_carries_every_setting() {
        Python::attach(|py| {
            let dir = tempfile::tempdir().unwrap();
            let half_life = 1.5f64.into_pyobject(py).unwrap().into_any();
            let cfg = PySessionConfig::oxigraph(
                py,
                dir.path().to_owned(),
                Some(Py::new(py, PyScopeBounds::new(Some(1), Some(2), None)).unwrap()),
                Some(PyEvictionPolicy::recency_weighted(&half_life).unwrap()),
                Some(PyDemotionTarget::NextColder),
                None,
            )
            .unwrap();
            cfg.promotion.borrow_mut(py).0.minor_to_major_accesses = 9;
            cfg.bounds.borrow_mut(py).minor_max_items = Some(3);

            let native = cfg.to_native(py).unwrap();
            assert_eq!(
                native.bounds,
                ctx::ScopeBounds {
                    enduring_max_items: Some(1),
                    major_max_items: Some(2),
                    minor_max_items: Some(3),
                }
            );
            assert_eq!(
                native.eviction_policy,
                ctx::EvictionPolicy::RecencyWeighted {
                    half_life: Duration::from_millis(1500)
                }
            );
            assert_eq!(native.demotion_target, ctx::DemotionTarget::NextColder);
            assert_eq!(native.promotion.minor_to_major_accesses, 9);
            assert_eq!(native.promotion.minor_window, Duration::from_secs(60));
        });
    }

    #[test]
    fn default_config_matches_the_rust_defaults() {
        Python::attach(|py| {
            let dir = tempfile::tempdir().unwrap();
            let cfg = PySessionConfig::oxigraph(py, dir.path().to_owned(), None, None, None, None)
                .unwrap();
            let native = cfg.to_native(py).unwrap();
            let other = tempfile::tempdir().unwrap();
            let rust = ctx::SessionConfig::oxigraph(other.path()).unwrap();
            assert_eq!(native.bounds, rust.bounds);
            assert_eq!(native.eviction_policy, rust.eviction_policy);
            assert_eq!(native.demotion_target, rust.demotion_target);
            assert_eq!(native.promotion, rust.promotion);
        });
    }

    #[test]
    fn eviction_criteria_build_their_rust_variants() {
        let s = PyEvictionCriteria::policy(2);
        assert_eq!(s.0, ctx::EvictionCriteria::Policy { count: 2 });
        let d = PyEvictionCriteria::demote(vec![], PyContextScope::Minor);
        assert_eq!(
            d.0,
            ctx::EvictionCriteria::Demote {
                items: vec![],
                to: ctx::ContextScope::Minor
            }
        );
    }
}
