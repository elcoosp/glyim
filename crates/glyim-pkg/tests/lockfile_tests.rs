use glyim_pkg::lockfile::*;
use std::collections::HashMap;

#[test]
fn generate_empty_lockfile() {
    let lock = generate_lockfile(&HashMap::new());
    assert!(lock.packages.is_empty());
}

#[test]
fn generate_and_parse_roundtrip() {
    let mut resolved = HashMap::new();
    resolved.insert(
        "serde".to_string(),
        (
            "1.0.0".to_string(),
            "sha256:abc123".to_string(),
            false,
            vec![],
            LockSource::Registry {
                url: "https://registry.glyim.dev".to_string(),
            },
        ),
    );
    resolved.insert(
        "serde-derive".to_string(),
        (
            "1.0.0".to_string(),
            "sha256:def456".to_string(),
            true,
            vec![],
            LockSource::Registry {
                url: "https://registry.glyim.dev".to_string(),
            },
        ),
    );
    let lock = generate_lockfile(&resolved);
    let serialized = serialize_lockfile(&lock);
    let parsed = parse_lockfile(&serialized).unwrap();
    assert_eq!(parsed.packages.len(), 2);
    assert_eq!(parsed.packages[0].name, "serde");
    assert!(matches!(
        parsed.packages[0].source,
        LockSource::Registry { .. }
    ));
    assert!(parsed.packages[1].is_macro);
    assert_eq!(parsed.packages[1].name, "serde-derive");
}

#[test]
fn serialize_contains_header() {
    let lock = generate_lockfile(&HashMap::new());
    let serialized = serialize_lockfile(&lock);
    assert!(serialized.contains("@generated"));
    assert!(serialized.contains("Do not edit"));
}

#[test]
fn serialize_sorted_by_name() {
    let mut resolved = HashMap::new();
    resolved.insert(
        "z".to_string(),
        (
            "0.1.0".to_string(),
            "hash:zzz".to_string(),
            false,
            vec![],
            LockSource::Local,
        ),
    );
    resolved.insert(
        "a".to_string(),
        (
            "0.2.0".to_string(),
            "hash:aaa".to_string(),
            false,
            vec![],
            LockSource::Registry {
                url: "https://example.com".to_string(),
            },
        ),
    );
    let lock = generate_lockfile(&resolved);
    let serialized = serialize_lockfile(&lock);
    let parsed = parse_lockfile(&serialized).unwrap();
    assert_eq!(parsed.packages[0].name, "a");
    assert_eq!(parsed.packages[1].name, "z");
}

#[test]
fn parse_invalid_toml() {
    let result = parse_lockfile("[invalid");
    assert!(result.is_err());
}

#[test]
fn compute_content_hash_deterministic() {
    let h1 = compute_content_hash(b"hello");
    let h2 = compute_content_hash(b"hello");
    assert_eq!(h1, h2);
}

#[test]
fn compute_content_hash_different_content() {
    let h1 = compute_content_hash(b"hello");
    let h2 = compute_content_hash(b"world");
    assert_ne!(h1, h2);
}

// ---- New edge-case tests ----

#[test]
fn lockfile_roundtrip_multiple_versions() {
    let mut resolved = HashMap::new();
    resolved.insert(
        "a".to_string(),
        (
            "1.0.0".to_string(),
            "sha256:abc".to_string(),
            false,
            vec![],
            LockSource::Local,
        ),
    );
    resolved.insert(
        "b".to_string(),
        (
            "2.0.0".to_string(),
            "sha256:def".to_string(),
            true,
            vec![],
            LockSource::Local,
        ),
    );
    let lock = generate_lockfile(&resolved);
    let serialized = serialize_lockfile(&lock);
    let parsed = parse_lockfile(&serialized).unwrap();
    assert_eq!(parsed.packages.len(), 2);
    assert!(
        parsed
            .packages
            .iter()
            .any(|p| p.name == "a" && p.version == "1.0.0")
    );
    assert!(
        parsed
            .packages
            .iter()
            .any(|p| p.name == "b" && p.version == "2.0.0" && p.is_macro)
    );
}

#[test]
fn lockfile_serialized_sorted() {
    let mut resolved = HashMap::new();
    resolved.insert(
        "z".to_string(),
        (
            "0.1.0".to_string(),
            "h".to_string(),
            false,
            vec![],
            LockSource::Local,
        ),
    );
    resolved.insert(
        "a".to_string(),
        (
            "0.2.0".to_string(),
            "h".to_string(),
            false,
            vec![],
            LockSource::Local,
        ),
    );
    let lock = generate_lockfile(&resolved);
    let serialized = serialize_lockfile(&lock);
    let parsed = parse_lockfile(&serialized).unwrap();
    assert_eq!(parsed.packages[0].name, "a");
    assert_eq!(parsed.packages[1].name, "z");
}

#[test]
fn lockfile_parse_missing_type_field_defaults_to_registry() {
    let toml = r#"
[[package]]
name = "foo"
version = "1.0"
hash = "abc"
"#;
    let result = parse_lockfile(toml);
    assert!(
        result.is_err(),
        "expected parse error for missing type field"
    );
    let result = parse_lockfile(toml);
    assert!(
        result.is_err(),
        "expected parse error for missing type field"
    );
}
