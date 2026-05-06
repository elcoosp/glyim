use glyim_query::result::{QueryResult, QueryStatus};
use glyim_query::fingerprint::Fingerprint;
use glyim_query::dependency::Dependency;
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn query_status_green_is_valid() {
    let status = QueryStatus::Green;
    assert!(status.is_valid());
}

#[test]
fn query_status_red_is_not_valid() {
    let status = QueryStatus::Red;
    assert!(!status.is_valid());
}

#[test]
fn query_result_stores_value() {
    let value: Arc<dyn Send + Sync> = Arc::new(42i64);
    let result = QueryResult::new(
        value,
        Fingerprint::of(b"42"),
        vec![Dependency::file("main.g", Fingerprint::of(b"src"))],
        QueryStatus::Green,
    );
    assert_eq!(result.fingerprint, Fingerprint::of(b"42"));
    assert_eq!(result.dependencies.len(), 1);
    assert!(result.status.is_valid());
}

#[test]
fn query_result_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<QueryResult>();
}

#[test]
fn query_result_downcast() {
    let value: Arc<dyn Send + Sync> = Arc::new(99i64);
    let result = QueryResult::new(
        value,
        Fingerprint::of(b"99"),
        vec![],
        QueryStatus::Green,
    );
    let downcast: Option<&i64> = result.value.downcast_ref::<i64>();
    assert_eq!(*downcast.unwrap(), 99i64);
}

#[test]
fn query_result_downcast_wrong_type_returns_none() {
    let value: Arc<dyn Send + Sync> = Arc::new(99i64);
    let result = QueryResult::new(
        value,
        Fingerprint::of(b"99"),
        vec![],
        QueryStatus::Green,
    );
    let downcast: Option<&String> = result.value.downcast_ref::<String>();
    assert!(downcast.is_none());
}
