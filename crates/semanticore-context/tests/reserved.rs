//! Surfaces RFC 0004 reserves at v0.1: SHACL and Datalog refuse with a
//! premium-only error; reasoning is not yet implemented in the OSS tier.

mod common;

use common::*;
use semanticore_context::{ContextError, ContextScope, SessionError, Unimplemented};
use semanticore_core::datalog::DatalogProgram;

#[test]
fn validate_is_premium_only() {
    let (_dir, s) = session();
    let err = s
        .validate(ContextScope::Enduring, ContextScope::Major)
        .unwrap_err();
    assert!(matches!(
        err,
        SessionError::PremiumOnly(ContextError::ShaclUnavailable)
    ));
    assert!(err.to_string().contains("premium"), "{err}");
}

#[test]
fn datalog_is_premium_only() {
    let (_dir, s) = session();
    let err = s
        .evaluate_datalog(&DatalogProgram::default(), ContextScope::Enduring)
        .unwrap_err();
    assert!(matches!(
        err,
        SessionError::PremiumOnly(ContextError::DatalogUnavailable)
    ));
    assert!(err.to_string().contains("premium"), "{err}");
}

#[test]
fn reason_is_not_yet_implemented_for_every_scope() {
    let (_dir, mut s) = session();
    for scope in ContextScope::ALL {
        let err = s.reason(scope).unwrap_err();
        assert!(matches!(
            err,
            SessionError::NotYetImplemented(Unimplemented::OwlElReasoner)
        ));
        assert!(err.to_string().contains("not yet implemented"), "{err}");
    }
    assert_eq!(s.close().unwrap().reasoning_passes, 0);
}

#[test]
fn not_yet_implemented_and_premium_only_are_distinct() {
    let nyi = SessionError::NotYetImplemented(Unimplemented::AnnIndex);
    let premium = SessionError::PremiumOnly(ContextError::ShaclUnavailable);
    assert!(!nyi.to_string().contains("premium"));
    assert!(!premium.to_string().contains("not yet implemented"));
    assert!(Unimplemented::Sparql.to_string().contains("SPARQL"));
}
