use std::any::Any;
use crate::context::QueryContext;
use crate::fingerprint::Fingerprint;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[test]
fn query_caches_result_on_first_call() {
    let ctx = QueryContext::new();
    let key = Fingerprint::of(b"test_key");
    let value: Arc<dyn Any + Send + Sync> = Arc::new(42i64);
    ctx.insert(key, value, Fingerprint::of(b"42"), vec![]);
    let result = ctx.get(&key);
    assert!(result.is_some());
    let r = result.unwrap();
    assert_eq!(*r.value.downcast_ref::<i64>().unwrap(), 42i64);
}

#[test]
fn query_returns_none_for_unknown_key() {
    let ctx = QueryContext::new();
    let key = Fingerprint::of(b"nonexistent");
    assert!(ctx.get(&key).is_none());
}

#[test]
fn query_overwrites_existing_result() {
    let ctx = QueryContext::new();
    let key = Fingerprint::of(b"key");
    ctx.insert(key, Arc::new(1i64), Fingerprint::of(b"1"), vec![]);
    ctx.insert(key, Arc::new(2i64), Fingerprint::of(b"2"), vec![]);
    let r = ctx.get(&key).unwrap();
    assert_eq!(*r.value.downcast_ref::<i64>().unwrap(), 2i64);
}

#[test]
fn query_records_dependencies() {
    let ctx = QueryContext::new();
    let key = Fingerprint::of(b"query");
    let dep = crate::Dependency::file("main.g", Fingerprint::of(b"src"));
    ctx.insert(key, Arc::new(42i64), Fingerprint::of(b"42"), vec![dep]);
    let r = ctx.get(&key).unwrap();
    assert_eq!(r.dependencies.len(), 1);
}

#[test]
fn query_invalidate_marks_red() {
    let ctx = QueryContext::new();
    let key = Fingerprint::of(b"query");
    ctx.insert(key, Arc::new(42i64), Fingerprint::of(b"42"), vec![]);
    assert!(ctx.get(&key).unwrap().is_valid());
    ctx.invalidate_key(key);
    assert!(!ctx.get(&key).unwrap().is_valid());
}

#[test]
fn query_invalidate_via_graph() {
    let ctx = QueryContext::new();
    let query_fp = Fingerprint::of(b"query_fingerprint");
    let dep = crate::Dependency::file("main.g", Fingerprint::of(b"src"));
    let dep_fp = dep.fingerprint();
    ctx.dep_graph().write().unwrap().add_node(dep_fp);
    ctx.insert(query_fp, Arc::new(42i64), Fingerprint::of(b"42"), vec![dep.clone()]);
    ctx.record_dependency(query_fp, dep);
    let report = ctx.invalidate_fingerprints(&[dep_fp]);
    assert!(report.red.contains(&query_fp));
}

#[test]
fn query_is_green_after_insert() {
    let ctx = QueryContext::new();
    let key = Fingerprint::of(b"q");
    ctx.insert(key, Arc::new(1i64), Fingerprint::of(b"1"), vec![]);
    assert!(ctx.is_green(&key));
}

#[test]
fn query_is_red_after_invalidation() {
    let ctx = QueryContext::new();
    let key = Fingerprint::of(b"q");
    ctx.insert(key, Arc::new(1i64), Fingerprint::of(b"1"), vec![]);
    ctx.invalidate_key(key);
    assert!(!ctx.is_green(&key));
}

#[test]
fn query_method_calls_compute_on_first_call() {
    let ctx = QueryContext::new();
    let call_count = Arc::new(AtomicU32::new(0));
    let count_clone = call_count.clone();
    let key = Fingerprint::of(b"my_query");
    let result: i64 = ctx.query(
        key,
        || {
            count_clone.fetch_add(1, Ordering::SeqCst);
            Arc::new(42i64) as Arc<dyn Any + Send + Sync>
        },
        Fingerprint::of(b"42"),
        vec![],
    );
    assert_eq!(result, 42i64);
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[test]
fn query_method_reuses_cache_on_second_call() {
    let ctx = QueryContext::new();
    let call_count = Arc::new(AtomicU32::new(0));
    let count_clone = call_count.clone();
    let key = Fingerprint::of(b"my_query");
    let _: i64 = ctx.query(
        key,
        || {
            count_clone.fetch_add(1, Ordering::SeqCst);
            Arc::new(42i64) as Arc<dyn Any + Send + Sync>
        },
        Fingerprint::of(b"42"),
        vec![],
    );
    let result: i64 = ctx.query(
        key,
        || {
            count_clone.fetch_add(1, Ordering::SeqCst);
            Arc::new(99i64) as Arc<dyn Any + Send + Sync>
        },
        Fingerprint::of(b"99"),
        vec![],
    );
    assert_eq!(result, 42i64);
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[test]
fn query_method_recomputes_after_invalidation() {
    let ctx = QueryContext::new();
    let call_count = Arc::new(AtomicU32::new(0));
    let count_clone = call_count.clone();
    let key = Fingerprint::of(b"my_query");
    let result1: i64 = ctx.query(
        key,
        || {
            count_clone.fetch_add(1, Ordering::SeqCst);
            Arc::new(42i64) as Arc<dyn Any + Send + Sync>
        },
        Fingerprint::of(b"42"),
        vec![],
    );
    assert_eq!(result1, 42i64);
    ctx.invalidate_key(key);
    let result2: i64 = ctx.query(
        key,
        || {
            count_clone.fetch_add(1, Ordering::SeqCst);
            Arc::new(100i64) as Arc<dyn Any + Send + Sync>
        },
        Fingerprint::of(b"100"),
        vec![],
    );
    assert_eq!(result2, 100i64);
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}
