use glyim_query::dependency::Dependency;
use glyim_query::fingerprint::Fingerprint;
use std::path::PathBuf;

#[test]
fn dependency_file_equality() {
    let hash = Fingerprint::of(b"abc");
    let a = Dependency::File { path: PathBuf::from("foo.g"), hash };
    let b = Dependency::File { path: PathBuf::from("foo.g"), hash };
    assert_eq!(a, b);
}

#[test]
fn dependency_file_different_path() {
    let hash = Fingerprint::of(b"abc");
    let a = Dependency::File { path: PathBuf::from("foo.g"), hash };
    let b = Dependency::File { path: PathBuf::from("bar.g"), hash };
    assert_ne!(a, b);
}

#[test]
fn dependency_file_different_hash() {
    let a = Dependency::File { path: PathBuf::from("foo.g"), hash: Fingerprint::of(b"abc") };
    let b = Dependency::File { path: PathBuf::from("foo.g"), hash: Fingerprint::of(b"def") };
    assert_ne!(a, b);
}

#[test]
fn dependency_query_equality() {
    let hash = Fingerprint::of(b"query_result");
    let a = Dependency::Query { key_fingerprint: hash };
    let b = Dependency::Query { key_fingerprint: hash };
    assert_eq!(a, b);
}

#[test]
fn dependency_config_equality() {
    let a = Dependency::Config { key: "opt_level".to_string(), value: Fingerprint::of(b"2") };
    let b = Dependency::Config { key: "opt_level".to_string(), value: Fingerprint::of(b"2") };
    assert_eq!(a, b);
}

#[test]
fn dependency_is_send_sync() {
    fn assert_bounds<T: Send + Sync>() {}
    assert_bounds::<Dependency>();
}

#[test]
fn dependency_variants_are_distinct() {
    let hash = Fingerprint::of(b"abc");
    let file = Dependency::File { path: PathBuf::from("x"), hash };
    let query = Dependency::Query { key_fingerprint: hash };
    let config = Dependency::Config { key: "k".to_string(), value: hash };
    assert_ne!(file, query);
    assert_ne!(file, config);
    assert_ne!(query, config);
}
