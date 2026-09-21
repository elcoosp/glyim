//! Regression tests for de-stubbing plan §21.5: real SemVer 2.0 matching in
//! `CrateIndex::resolve_version` (replaces naive `starts_with(req)` prefix
//! matching). Covers caret, tilde, wildcard, comparison operators, and exact
//! requirements, plus the historical unparseable-req → latest fallback.

use crate::dep::{CrateIndex, IndexEntry};
use std::collections::HashMap;

fn index_with(name: &str, _unused: &[&str]) -> CrateIndex {
    let versions: Vec<String> = [
        "5.0.0", "4.1.0", "4.0.0", "3.2.1", "3.0.0", "2.5.0", "2.0.0", "1.9.9", "1.2.5", "1.2.0",
        "1.0.0",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        dependencies: HashMap::new(),
        name: name.to_string(),
        versions,
        checksums: HashMap::new(),
    });
    index
}

#[test]
fn resolve_caret_picks_highest_compatible() {
    // ^1.2.0 means >=1.2.0, <2.0.0 → highest 1.x is 1.9.9.
    let index = index_with("foo", &[]);
    let v = index
        .resolve_version("foo", Some("^1.2.0"))
        .expect("resolved");
    assert_eq!(v, "1.9.9");
}

#[test]
fn resolve_caret_excludes_next_major() {
    // ^4.0.0 means >=4.0.0, <5.0.0 → 4.1.0, never 5.0.0.
    let index = index_with("foo", &[]);
    let v = index
        .resolve_version("foo", Some("^4.0.0"))
        .expect("resolved");
    assert_eq!(v, "4.1.0");
}

#[test]
fn resolve_tilde_picks_highest_patch() {
    // ~1.2.0 means >=1.2.0, <1.3.0 → highest patch in 1.2.x is 1.2.5.
    let index = index_with("foo", &[]);
    let v = index
        .resolve_version("foo", Some("~1.2.0"))
        .expect("resolved");
    assert_eq!(v, "1.2.5");
}

#[test]
fn resolve_wildcard_picks_highest_minor() {
    // 1.2.* means >=1.2.0, <1.3.0 → 1.2.5.
    let index = index_with("foo", &[]);
    let v = index
        .resolve_version("foo", Some("1.2.*"))
        .expect("resolved");
    assert_eq!(v, "1.2.5");
}

#[test]
fn resolve_comparator_range() {
    // >=1.0.0, <2.0.0 → highest 1.x is 1.9.9.
    let index = index_with("foo", &[]);
    let v = index
        .resolve_version("foo", Some(">=1.0.0, <2.0.0"))
        .expect("resolved");
    assert_eq!(v, "1.9.9");
}

#[test]
fn resolve_exact_version() {
    // =3.2.1 (or bare 3.2.1) matches exactly.
    let index = index_with("foo", &[]);
    let v = index
        .resolve_version("foo", Some("=3.2.1"))
        .expect("resolved");
    assert_eq!(v, "3.2.1");
    let v2 = index
        .resolve_version("foo", Some("3.2.1"))
        .expect("resolved");
    assert_eq!(v2, "3.2.1");
}

#[test]
fn resolve_no_req_uses_latest() {
    let index = index_with("foo", &[]);
    let v = index.resolve_version("foo", None).expect("resolved");
    assert_eq!(v, "5.0.0");
}

#[test]
fn resolve_unparseable_or_unmatched_req_is_not_found() {
    // `99` parses as `^99.0.0` but no such version exists → must error,
    // not silently fall back to latest (that was the stub-era behavior).
    let index = index_with("foo", &[]);
    let res = index.resolve_version("foo", Some("99"));
    assert!(res.is_err(), "`99` has no matching version → must error");
}

#[test]
fn resolve_parseable_but_unmatched_req_is_not_found() {
    // A valid requirement with no satisfying version must error, not fall back.
    let index = index_with("foo", &[]);
    let res = index.resolve_version("foo", Some("^9.0.0"));
    assert!(res.is_err(), "no 9.x version exists → must error");
}

#[test]
fn resolve_unknown_crate_is_not_found() {
    let index = index_with("foo", &[]);
    let res = index.resolve_version("nope", Some("^1.0.0"));
    assert!(res.is_err());
}
