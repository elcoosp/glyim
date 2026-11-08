//! Advanced lockfile tests — edge cases.

use crate::lockfile::*;
use std::collections::BTreeMap;

#[test]
fn lockfile_default_is_empty() {
    let lf = Lockfile::default();
    assert!(lf.is_empty());
    assert_eq!(lf.version, 1);
}

#[test]
fn lockfile_add_duplicate_overwrites() {
    let mut lf = Lockfile::new();
    lf.add_crate(LockedCrate {
        name: "foo".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Path {
            path: "/original".to_string(),
        },
        dependencies: BTreeMap::new(),
    });
    lf.add_crate(LockedCrate {
        name: "foo".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Path {
            path: "/updated".to_string(),
        },
        dependencies: BTreeMap::new(),
    });
    assert_eq!(lf.len(), 1);
    let locked = lf.get_crate("foo", "1.0.0").expect("find");
    match &locked.source {
        CrateSource::Path { path } => assert_eq!(path, "/updated"),
        _ => panic!("expected Path source"),
    }
}

#[test]
fn lockfile_get_nonexistent_crate() {
    let lf = Lockfile::new();
    assert!(lf.get_crate("missing", "1.0").is_none());
}

#[test]
fn locked_crate_with_dependencies() {
    let mut lf = Lockfile::new();
    let mut deps = BTreeMap::new();
    deps.insert("core".to_string(), "1.0.0".to_string());
    deps.insert("alloc".to_string(), "1.0.0".to_string());

    lf.add_crate(LockedCrate {
        name: "my-crate".to_string(),
        version: "2.0.0".to_string(),
        source: CrateSource::Registry {
            url: "https://index.glyim.dev".to_string(),
            checksum: "sha256:abcd".to_string(),
        },
        dependencies: deps,
    });

    let locked = lf.get_crate("my-crate", "2.0.0").expect("find");
    assert_eq!(locked.dependencies.len(), 2);
    assert_eq!(locked.dependencies.get("core"), Some(&"1.0.0".to_string()));
    assert_eq!(locked.dependencies.get("alloc"), Some(&"1.0.0".to_string()));
}

#[test]
fn crate_source_git_no_optional_fields() {
    let src = CrateSource::Git {
        url: "https://git.example.com/repo".to_string(),
        rev: None,
        branch: None,
        tag: None,
    };
    let ser = toml::to_string(&src).expect("serialize");
    // Optional fields should not appear when None
    assert!(!ser.contains("rev"));
    assert!(!ser.contains("branch"));
    assert!(!ser.contains("tag"));
    assert!(ser.contains("url"));
}

#[test]
fn lockfile_parse_invalid_toml() {
    let result = Lockfile::parse("this is not valid {{{}}");
    assert!(result.is_err());
}

#[test]
fn lockfile_empty_crates_iterator() {
    let lf = Lockfile::new();
    let count = lf.crates().count();
    assert_eq!(count, 0);
}

#[test]
fn lockfile_multiple_crates_iteration_order() {
    let mut lf = Lockfile::new();
    for name in &["aaa", "bbb", "ccc"] {
        lf.add_crate(LockedCrate {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            source: CrateSource::Path {
                path: format!("/{name}"),
            },
            dependencies: BTreeMap::new(),
        });
    }
    // BTreeMap should give sorted order
    let names: Vec<String> = lf.crates().map(|c| c.name.clone()).collect();
    assert_eq!(names, vec!["aaa", "bbb", "ccc"]);
}
