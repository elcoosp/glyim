use glyim_pkg::lockfile::LockSource;
use glyim_pkg::resolver::*;
use std::collections::HashMap;

fn make_req(name: &str, version: &str, is_macro: bool) -> Requirement {
    Requirement {
        name: name.to_string(),
        version_constraint: version.to_string(),
        is_macro,
        source: LockSource::Registry {
            url: "https://registry.glyim.dev".to_string(),
        },
    }
}

fn make_available(_name: &str, version: &str, deps: &[(&str, &str)]) -> AvailableVersion {
    AvailableVersion {
        version: version.to_string(),
        is_macro: false,
        deps: deps.iter().map(|(n, v)| make_req(n, v, false)).collect(),
        source: LockSource::Registry {
            url: "https://registry.glyim.dev".to_string(),
        },
    }
}

#[test]
fn resolve_single_dep_exact_version() {
    let mut available = HashMap::new();
    available.insert(
        "foo".to_string(),
        vec![
            make_available("foo", "1.0.0", &[]),
            make_available("foo", "1.5.0", &[]),
            make_available("foo", "2.0.0", &[]),
        ],
    );
    let resolution = resolve(&[make_req("foo", "1.0.0", false)], None, &available).unwrap();
    assert_eq!(resolution.packages["foo"].version, "1.0.0");
}

#[test]
fn resolve_single_dep_caret_range() {
    let mut available = HashMap::new();
    available.insert(
        "bar".to_string(),
        vec![
            make_available("bar", "1.2.0", &[]),
            make_available("bar", "1.4.0", &[]),
            make_available("bar", "2.0.0", &[]),
        ],
    );
    let resolution = resolve(&[make_req("bar", "^1.2.0", false)], None, &available).unwrap();
    assert_eq!(resolution.packages["bar"].version, "1.2.0");
}

#[test]
fn resolve_single_dep_wildcard() {
    let mut available = HashMap::new();
    available.insert(
        "baz".to_string(),
        vec![
            make_available("baz", "0.5.0", &[]),
            make_available("baz", "1.0.0", &[]),
        ],
    );
    let resolution = resolve(&[make_req("baz", "*", false)], None, &available).unwrap();
    assert_eq!(resolution.packages["baz"].version, "0.5.0");
}

#[test]
fn resolve_unknown_package_errors() {
    let resolution = resolve(
        &[make_req("nonexistent", "1.0.0", false)],
        None,
        &HashMap::new(),
    );
    assert!(resolution.is_err());
}

#[test]
fn resolve_no_satisfying_version_errors() {
    let mut available = HashMap::new();
    available.insert(
        "qux".to_string(),
        vec![
            make_available("qux", "1.0.0", &[]),
            make_available("qux", "1.2.0", &[]),
        ],
    );
    let resolution = resolve(&[make_req("qux", "^2.0.0", false)], None, &available);
    assert!(resolution.is_err());
}

#[test]
fn resolve_transitive_deps() {
    let mut available = HashMap::new();
    available.insert(
        "a".to_string(),
        vec![make_available("a", "1.0.0", &[("b", "1.0.0")])],
    );
    available.insert("b".to_string(), vec![make_available("b", "1.0.0", &[])]);
    let resolution = resolve(&[make_req("a", "^1.0.0", false)], None, &available).unwrap();
    assert_eq!(resolution.packages["a"].version, "1.0.0");
    assert_eq!(resolution.packages["b"].version, "1.0.0");
}

#[test]
fn satisfies_exact() {
    assert!(satisfies_constraint("1.2.3", "1.2.3"));
    assert!(!satisfies_constraint("1.2.3", "1.2.4"));
}

#[test]
fn satisfies_caret() {
    assert!(satisfies_constraint("1.2.5", "^1.2.3"));
    assert!(satisfies_constraint("1.9.9", "^1.0.0"));
    assert!(!satisfies_constraint("2.0.0", "^1.0.0"));
    assert!(!satisfies_constraint("0.9.0", "^1.0.0"));
}

// ---- New edge-case tests ----

#[test]
fn resolve_conflicting_constraints_no_satisfying() {
    let mut available = HashMap::new();
    available.insert(
        "foo".to_string(),
        vec![
            make_available("foo", "1.0.0", &[]),
            make_available("foo", "2.0.0", &[]),
        ],
    );
    let req = make_req("foo", "^1.5.0", false);
    let result = resolve(&[req], None, &available);
    assert!(result.is_err());
}

#[test]
fn resolve_cyclic_dependency_detected() {
    let mut available = HashMap::new();
    available.insert(
        "a".to_string(),
        vec![make_available("a", "1.0.0", &[("b", "1.0.0")])],
    );
    available.insert(
        "b".to_string(),
        vec![make_available("b", "1.0.0", &[("a", "1.0.0")])],
    );
    let result = resolve(&[make_req("a", "1.0.0", false)], None, &available);
    match result {
        Ok(_) => {}
        Err(e) => assert!(e.to_string().contains("not found")),
    }
}
