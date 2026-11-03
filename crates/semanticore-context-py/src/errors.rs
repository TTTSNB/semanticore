//! Rust errors -> Python exception classes.
//!
//! Every [`semanticore_context::SessionError`] variant maps to its own
//! subclass of `SessionError`, with the variant's fields set as attributes
//! on the raised exception:
//!
//! ```text
//! SessionError(Exception)
//! ├── CapabilityShortfallError   .scope, .required
//! ├── MissingProvenanceError
//! ├── InvalidItemError           .reason
//! ├── UnknownItemError           .item_id
//! ├── ScopeMismatchError         .item_id, .expected, .found
//! ├── DurabilityViolationError   .from_scope, .to_scope
//! ├── PremiumOnlyError
//! │   ├── ShaclUnavailableError
//! │   └── DatalogUnavailableError
//! ├── NotYetImplementedError     .feature
//! ├── CorruptStoreError          .reason
//! ├── StorageError
//! └── SessionClosedError         (binding lifecycle, see below)
//! ```
//!
//! `SessionClosedError` is the one class with no Rust counterpart: a Rust
//! `Session` is consumed by `close`, but the Python object outlives it.

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use semanticore_context as ctx;

use crate::item::{PyContextScope, PyItemId};

create_exception!(
    semanticore_context,
    SessionError,
    PyException,
    "Base class for every error raised by a Session."
);
create_exception!(
    semanticore_context,
    CapabilityShortfallError,
    SessionError,
    "A scope's storage lacks a capability the scope needs."
);
create_exception!(
    semanticore_context,
    MissingProvenanceError,
    SessionError,
    "The item's provenance has an empty source."
);
create_exception!(
    semanticore_context,
    InvalidItemError,
    SessionError,
    "The item cannot be stored as given."
);
create_exception!(
    semanticore_context,
    UnknownItemError,
    SessionError,
    "No live item has this id."
);
create_exception!(
    semanticore_context,
    ScopeMismatchError,
    SessionError,
    "The item is not in the scope the operation names."
);
create_exception!(
    semanticore_context,
    DurabilityViolationError,
    SessionError,
    "Promotion to a less durable scope, or demotion to one that is not less durable."
);
create_exception!(
    semanticore_context,
    PremiumOnlyError,
    SessionError,
    "The operation is reserved for the premium tier at v0.1."
);
create_exception!(
    semanticore_context,
    ShaclUnavailableError,
    PremiumOnlyError,
    "SHACL validation needs a SHACL reasoner from the premium tier."
);
create_exception!(
    semanticore_context,
    DatalogUnavailableError,
    PremiumOnlyError,
    "Datalog evaluation needs a Datalog engine from the premium tier."
);
create_exception!(
    semanticore_context,
    NotYetImplementedError,
    SessionError,
    "The operation is specified by RFC 0004 but not implemented in this release."
);
create_exception!(
    semanticore_context,
    CorruptStoreError,
    SessionError,
    "Stored item metadata could not be decoded on open."
);
create_exception!(
    semanticore_context,
    StorageError,
    SessionError,
    "An error from the underlying storage."
);
create_exception!(
    semanticore_context,
    SessionClosedError,
    SessionError,
    "The Session has been closed."
);

/// Register every exception class on the module.
pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("SessionError", py.get_type::<SessionError>())?;
    m.add(
        "CapabilityShortfallError",
        py.get_type::<CapabilityShortfallError>(),
    )?;
    m.add(
        "MissingProvenanceError",
        py.get_type::<MissingProvenanceError>(),
    )?;
    m.add("InvalidItemError", py.get_type::<InvalidItemError>())?;
    m.add("UnknownItemError", py.get_type::<UnknownItemError>())?;
    m.add("ScopeMismatchError", py.get_type::<ScopeMismatchError>())?;
    m.add(
        "DurabilityViolationError",
        py.get_type::<DurabilityViolationError>(),
    )?;
    m.add("PremiumOnlyError", py.get_type::<PremiumOnlyError>())?;
    m.add(
        "ShaclUnavailableError",
        py.get_type::<ShaclUnavailableError>(),
    )?;
    m.add(
        "DatalogUnavailableError",
        py.get_type::<DatalogUnavailableError>(),
    )?;
    m.add(
        "NotYetImplementedError",
        py.get_type::<NotYetImplementedError>(),
    )?;
    m.add("CorruptStoreError", py.get_type::<CorruptStoreError>())?;
    m.add("StorageError", py.get_type::<StorageError>())?;
    m.add("SessionClosedError", py.get_type::<SessionClosedError>())?;
    Ok(())
}

/// The stable Python spelling of an [`ctx::Unimplemented`] feature, exposed
/// as `NotYetImplementedError.feature`.
pub(crate) fn feature_name(what: ctx::Unimplemented) -> &'static str {
    match what {
        ctx::Unimplemented::OwlElReasoner => "owl_el_reasoner",
        ctx::Unimplemented::AnnIndex => "ann_index",
        ctx::Unimplemented::Sparql => "sparql",
        // `Unimplemented` is non-exhaustive; a feature added upstream
        // before the binding learns its name still maps to the right class.
        _ => "unknown",
    }
}

/// Build the exception for `e`, with its fields set as attributes.
pub(crate) fn to_py_err(py: Python<'_>, e: ctx::SessionError) -> PyErr {
    let msg = e.to_string();
    let attrs: Vec<(&str, PyResult<Py<PyAny>>)>;
    let err = match e {
        ctx::SessionError::CapabilityShortfall { scope, required } => {
            attrs = vec![
                ("scope", scope_obj(py, scope)),
                ("required", str_obj(py, &format!("{required:?}"))),
            ];
            CapabilityShortfallError::new_err(msg)
        }
        ctx::SessionError::MissingProvenance => {
            attrs = vec![];
            MissingProvenanceError::new_err(msg)
        }
        ctx::SessionError::InvalidItem { reason } => {
            attrs = vec![("reason", str_obj(py, &reason))];
            InvalidItemError::new_err(msg)
        }
        ctx::SessionError::UnknownItem(id) => {
            attrs = vec![("item_id", id_obj(py, id))];
            UnknownItemError::new_err(msg)
        }
        ctx::SessionError::ScopeMismatch {
            item,
            expected,
            found,
        } => {
            attrs = vec![
                ("item_id", id_obj(py, item)),
                ("expected", scope_obj(py, expected)),
                ("found", scope_obj(py, found)),
            ];
            ScopeMismatchError::new_err(msg)
        }
        ctx::SessionError::DurabilityViolation { from, to } => {
            attrs = vec![
                ("from_scope", scope_obj(py, from)),
                ("to_scope", scope_obj(py, to)),
            ];
            DurabilityViolationError::new_err(msg)
        }
        ctx::SessionError::PremiumOnly(which) => {
            attrs = vec![];
            match which {
                ctx::ContextError::ShaclUnavailable => ShaclUnavailableError::new_err(msg),
                ctx::ContextError::DatalogUnavailable => DatalogUnavailableError::new_err(msg),
                _ => PremiumOnlyError::new_err(msg),
            }
        }
        ctx::SessionError::NotYetImplemented(what) => {
            attrs = vec![("feature", str_obj(py, feature_name(what)))];
            NotYetImplementedError::new_err(msg)
        }
        ctx::SessionError::Corrupt { reason } => {
            attrs = vec![("reason", str_obj(py, &reason))];
            CorruptStoreError::new_err(msg)
        }
        ctx::SessionError::Storage(_) => {
            attrs = vec![];
            StorageError::new_err(msg)
        }
        // `SessionError` is non-exhaustive.
        _ => {
            attrs = vec![];
            SessionError::new_err(msg)
        }
    };
    let value = err.value(py);
    for (name, v) in attrs {
        // Building or setting an attribute on a fresh exception only fails
        // on memory exhaustion; if it does, raise that instead.
        if let Err(attr_err) = v.and_then(|v| value.setattr(name, v)) {
            return attr_err;
        }
    }
    err
}

pub(crate) fn closed_err() -> PyErr {
    SessionClosedError::new_err("the Session is closed")
}

fn str_obj(py: Python<'_>, s: &str) -> PyResult<Py<PyAny>> {
    Ok(pyo3::types::PyString::new(py, s).into_any().unbind())
}

fn scope_obj(py: Python<'_>, scope: ctx::ContextScope) -> PyResult<Py<PyAny>> {
    Ok(Py::new(py, PyContextScope::from(scope))?.into_any())
}

fn id_obj(py: Python<'_>, id: ctx::ItemId) -> PyResult<Py<PyAny>> {
    Ok(Py::new(py, PyItemId(id))?.into_any())
}

#[cfg(test)]
mod tests {
    use super::*;
    use semanticore_core::storage::StorageCapability;

    /// `(error, expected class)` for every variant the binding names.
    fn cases() -> Vec<(ctx::SessionError, &'static str)> {
        use ctx::ContextScope::{Enduring, Major, Minor};
        vec![
            (
                ctx::SessionError::CapabilityShortfall {
                    scope: Enduring,
                    required: StorageCapability::Persistent,
                },
                "CapabilityShortfallError",
            ),
            (
                ctx::SessionError::MissingProvenance,
                "MissingProvenanceError",
            ),
            (
                ctx::SessionError::InvalidItem { reason: "r".into() },
                "InvalidItemError",
            ),
            (
                ctx::SessionError::DurabilityViolation {
                    from: Enduring,
                    to: Minor,
                },
                "DurabilityViolationError",
            ),
            (
                ctx::SessionError::PremiumOnly(ctx::ContextError::ShaclUnavailable),
                "ShaclUnavailableError",
            ),
            (
                ctx::SessionError::PremiumOnly(ctx::ContextError::DatalogUnavailable),
                "DatalogUnavailableError",
            ),
            (
                ctx::SessionError::NotYetImplemented(ctx::Unimplemented::Sparql),
                "NotYetImplementedError",
            ),
            (
                ctx::SessionError::Corrupt { reason: "r".into() },
                "CorruptStoreError",
            ),
            (
                ctx::SessionError::Storage(Box::new(std::io::Error::other("disk"))),
                "StorageError",
            ),
            (
                ctx::SessionError::ScopeMismatch {
                    item: first_id(),
                    expected: Minor,
                    found: Major,
                },
                "ScopeMismatchError",
            ),
            (
                ctx::SessionError::UnknownItem(first_id()),
                "UnknownItemError",
            ),
        ]
    }

    /// `ItemId` has no public constructor; get a real one from a Session.
    fn first_id() -> ctx::ItemId {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ctx::Session::open(ctx::SessionConfig::oxigraph(dir.path()).unwrap()).unwrap();
        s.add(
            ctx::ContextScope::Minor,
            ctx::ContextItem::chunk("x", "text/plain", ctx::Provenance::new("t")),
        )
        .unwrap()
    }

    #[test]
    fn every_variant_maps_to_its_own_session_error_subclass() {
        Python::attach(|py| {
            for (e, class) in cases() {
                let msg = e.to_string();
                let err = to_py_err(py, e);
                let ty = err.get_type(py);
                assert_eq!(ty.name().unwrap().to_string(), class);
                assert!(err.is_instance_of::<SessionError>(py), "{class}");
                assert_eq!(err.value(py).str().unwrap().to_string(), msg);
            }
        });
    }

    #[test]
    fn premium_only_errors_share_a_base_distinct_from_not_yet_implemented() {
        Python::attach(|py| {
            let shacl = to_py_err(
                py,
                ctx::SessionError::PremiumOnly(ctx::ContextError::ShaclUnavailable),
            );
            assert!(shacl.is_instance_of::<PremiumOnlyError>(py));
            assert!(!shacl.is_instance_of::<NotYetImplementedError>(py));
            let nyi = to_py_err(
                py,
                ctx::SessionError::NotYetImplemented(ctx::Unimplemented::AnnIndex),
            );
            assert!(!nyi.is_instance_of::<PremiumOnlyError>(py));
        });
    }

    #[test]
    fn variant_fields_become_attributes() {
        Python::attach(|py| {
            let attr = |err: &PyErr, name: &str| err.value(py).getattr(name).unwrap();

            let e = to_py_err(
                py,
                ctx::SessionError::DurabilityViolation {
                    from: ctx::ContextScope::Enduring,
                    to: ctx::ContextScope::Minor,
                },
            );
            assert_eq!(
                attr(&e, "from_scope").extract::<PyContextScope>().unwrap(),
                PyContextScope::Enduring
            );
            assert_eq!(
                attr(&e, "to_scope").extract::<PyContextScope>().unwrap(),
                PyContextScope::Minor
            );

            let id = first_id();
            let e = to_py_err(
                py,
                ctx::SessionError::ScopeMismatch {
                    item: id,
                    expected: ctx::ContextScope::Minor,
                    found: ctx::ContextScope::Major,
                },
            );
            assert_eq!(
                attr(&e, "item_id").extract::<PyItemId>().unwrap(),
                PyItemId(id)
            );
            assert_eq!(
                attr(&e, "found").extract::<PyContextScope>().unwrap(),
                PyContextScope::Major
            );

            let e = to_py_err(
                py,
                ctx::SessionError::CapabilityShortfall {
                    scope: ctx::ContextScope::Enduring,
                    required: StorageCapability::Persistent,
                },
            );
            assert_eq!(
                attr(&e, "required").extract::<String>().unwrap(),
                "Persistent"
            );

            for (what, name) in [
                (ctx::Unimplemented::OwlElReasoner, "owl_el_reasoner"),
                (ctx::Unimplemented::AnnIndex, "ann_index"),
                (ctx::Unimplemented::Sparql, "sparql"),
            ] {
                let e = to_py_err(py, ctx::SessionError::NotYetImplemented(what));
                assert_eq!(attr(&e, "feature").extract::<String>().unwrap(), name);
            }

            let e = to_py_err(
                py,
                ctx::SessionError::InvalidItem {
                    reason: "why".into(),
                },
            );
            assert_eq!(attr(&e, "reason").extract::<String>().unwrap(), "why");
        });
    }

    #[test]
    fn session_closed_is_a_session_error() {
        Python::attach(|py| {
            let e = closed_err();
            assert!(e.is_instance_of::<SessionClosedError>(py));
            assert!(e.is_instance_of::<SessionError>(py));
        });
    }
}
