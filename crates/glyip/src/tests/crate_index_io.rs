//! Tests for CrateIndex loading from and saving to directory — S23.

use crate::dep::{CrateIndex, IndexEntry};
use std::collections::HashMap;
use tempfile::TempDir;

#[test]
fn crate_index_load_from_empty_dir() {
    let dir = TempDir::new().unwrap();
    let index = CrateIndex::load_from_dir(dir.path()).unwrap();
    assert!(index.is_empty());
}

#[test]
fn crate_index_load_from_nonexistent_dir() {
    let index = CrateIndex::load_from_dir(std::path::Path::new("/nonexistent/xyz123abc")).unwrap();
    assert!(index.is_empty());
}

#[test]
fn crate_index_save_and_load_roundtrip() {
    let dir = TempDir::new().unwrap();
    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "test-crate".to_string(),
        versions: vec!["1.0.0".to_string(), "0.9.0".to_string()],
        checksums: {
            let mut m = HashMap::new();
            m.insert("1.0.0".to_string(), "sha256:abc".to_string());
            m
        },
    });
    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "other-crate".to_string(),
        versions: vec!["2.0.0".to_string()],
        checksums: HashMap::new(),
    });

    index.save_to_dir(dir.path()).unwrap();
    let loaded = CrateIndex::load_from_dir(dir.path()).unwrap();

    assert_eq!(loaded.len(), 2);
    let entry = loaded.get("test-crate").unwrap();
    assert_eq!(
        entry.versions,
        vec!["1.0.0".to_string(), "0.9.0".to_string()]
    );
    assert_eq!(entry.checksums.get("1.0.0").unwrap(), "sha256:abc");

    let other = loaded.get("other-crate").unwrap();
    assert_eq!(other.versions, vec!["2.0.0".to_string()]);
}

#[test]
fn crate_index_save_creates_json_files() {
    let dir = TempDir::new().unwrap();
    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "mylib".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: HashMap::new(),
    });

    index.save_to_dir(dir.path()).unwrap();
    assert!(dir.path().join("mylib.json").exists());
}

#[test]
fn crate_index_load_ignores_non_json_files() {
    let dir = TempDir::new().unwrap();
    let entry = IndexEntry {
        dependencies: Default::default(),
        name: "valid".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: HashMap::new(),
    };
    std::fs::write(
        dir.path().join("valid.json"),
        serde_json::to_string(&entry).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.path().join("readme.txt"), "not json").unwrap();

    let index = CrateIndex::load_from_dir(dir.path()).unwrap();
    assert_eq!(index.len(), 1);
    assert!(index.get("valid").is_some());
}

#[test]
fn crate_index_load_handles_invalid_json() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("bad.json"), "not valid json {{{}").unwrap();

    let result = CrateIndex::load_from_dir(dir.path());
    assert!(result.is_err(), "invalid JSON should return an error");
}

#[test]
fn crate_index_save_empty_index() {
    let dir = TempDir::new().unwrap();
    let index = CrateIndex::new();
    index.save_to_dir(dir.path()).unwrap();
    let json_files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    assert!(json_files.is_empty());
}

#[test]
fn crate_index_save_overwrites_existing() {
    let dir = TempDir::new().unwrap();

    let mut index1 = CrateIndex::new();
    index1.insert(IndexEntry {
        dependencies: Default::default(),
        name: "lib".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: HashMap::new(),
    });
    index1.save_to_dir(dir.path()).unwrap();

    let mut index2 = CrateIndex::new();
    index2.insert(IndexEntry {
        dependencies: Default::default(),
        name: "lib".to_string(),
        versions: vec!["2.0.0".to_string()],
        checksums: HashMap::new(),
    });
    index2.save_to_dir(dir.path()).unwrap();

    let loaded = CrateIndex::load_from_dir(dir.path()).unwrap();
    let entry = loaded.get("lib").unwrap();
    assert_eq!(entry.versions, vec!["2.0.0".to_string()]);
}

#[test]
fn crate_index_len_and_is_empty() {
    let mut index = CrateIndex::new();
    assert!(index.is_empty());
    assert_eq!(index.len(), 0);

    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "foo".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: HashMap::new(),
    });
    assert!(!index.is_empty());
    assert_eq!(index.len(), 1);

    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "bar".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: HashMap::new(),
    });
    assert_eq!(index.len(), 2);
}
