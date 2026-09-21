//! Advanced dependency resolution tests.

use crate::config::*;
use crate::dep::*;
use crate::lockfile::{CrateSource, LockedCrate, Lockfile};
use std::collections::{BTreeMap, HashMap};
use tempfile::TempDir;

fn make_config_with_deps(
    name: &str,
    deps: BTreeMap<String, Dependency>,
    dev_deps: BTreeMap<String, Dependency>,
) -> GlyipToml {
    GlyipToml {
        package: PackageConfig {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            edition: "2024".to_string(),
            authors: Vec::new(),
            description: None,
            bin: None,
            lib: None,
        },
        dependencies: deps,
        dev_dependencies: dev_deps,
    }
}

#[test]
fn resolve_dev_dependencies() {
    let dir = TempDir::new().expect("temp dir");
    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "test-util".to_string(),
        versions: vec!["0.1.0".to_string()],
        checksums: HashMap::new(),
    });

    let dev_deps = {
        let mut m = BTreeMap::new();
        m.insert(
            "test-util".to_string(),
            Dependency::Simple("0.1".to_string()),
        );
        m
    };

    let config = make_config_with_deps("main", BTreeMap::new(), dev_deps);
    let resolver = DependencyResolver::new(index);
    let lockfile = resolver.resolve(&config, dir.path()).expect("resolve");

    assert_eq!(lockfile.len(), 1);
    let locked = lockfile.get_crate("test-util", "0.1.0").expect("find");
    assert_eq!(locked.name, "test-util");
}

#[test]
fn resolve_multiple_dependencies() {
    let dir = TempDir::new().expect("temp dir");
    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "dep-a".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: {
            let mut m = HashMap::new();
            m.insert("1.0.0".to_string(), "sha256:aaa".to_string());
            m
        },
    });
    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "dep-b".to_string(),
        versions: vec!["2.0.0".to_string()],
        checksums: {
            let mut m = HashMap::new();
            m.insert("2.0.0".to_string(), "sha256:bbb".to_string());
            m
        },
    });

    let deps = {
        let mut m = BTreeMap::new();
        m.insert("dep-a".to_string(), Dependency::Simple("1.0".to_string()));
        m.insert("dep-b".to_string(), Dependency::Simple("2.0".to_string()));
        m
    };

    let config = make_config_with_deps("main", deps, BTreeMap::new());
    let resolver = DependencyResolver::new(index);
    let lockfile = resolver.resolve(&config, dir.path()).expect("resolve");

    assert_eq!(lockfile.len(), 2);
}

#[test]
fn resolve_deduplicates_same_dep() {
    let dir = TempDir::new().expect("temp dir");
    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "shared".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: HashMap::new(),
    });

    // Both dependencies and dev_dependencies list "shared"
    let deps = {
        let mut m = BTreeMap::new();
        m.insert("shared".to_string(), Dependency::Simple("1.0".to_string()));
        m
    };
    let dev_deps = {
        let mut m = BTreeMap::new();
        m.insert("shared".to_string(), Dependency::Simple("1.0".to_string()));
        m
    };

    let config = make_config_with_deps("main", deps, dev_deps);
    let resolver = DependencyResolver::new(index);
    let lockfile = resolver.resolve(&config, dir.path()).expect("resolve");

    // Should only appear once
    assert_eq!(lockfile.len(), 1);
}

#[test]
fn crate_index_insert_and_get() {
    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "mylib".to_string(),
        versions: vec!["3.0.0".to_string()],
        checksums: HashMap::new(),
    });

    let entry = index.get("mylib").expect("find");
    assert_eq!(entry.name, "mylib");
    assert_eq!(entry.versions, vec!["3.0.0".to_string()]);
}

#[test]
fn crate_index_missing_entry() {
    let index = CrateIndex::new();
    assert!(index.get("missing").is_none());
}

#[test]
fn resolve_version_no_match_is_not_found() {
    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "foo".to_string(),
        versions: vec!["5.0.0".to_string(), "4.0.0".to_string()],
        checksums: HashMap::new(),
    });

    // "99" parses as `^99.0.0` but no such version exists → real SemVer
    // (plan §21.5) must error rather than silently fall back to latest.
    let res = index.resolve_version("foo", Some("99"));
    assert!(res.is_err(), "`99` has no matching version → must error");
}

#[test]
fn detect_self_cycle() {
    let mut lf = Lockfile::new();
    lf.add_crate(LockedCrate {
        name: "self-cycle".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Path {
            path: "/sc".to_string(),
        },
        dependencies: {
            let mut m = BTreeMap::new();
            m.insert("self-cycle".to_string(), "1.0.0".to_string());
            m
        },
    });

    let resolver = DependencyResolver::new_no_index();
    let result = resolver.detect_cycles(&lf);
    assert!(result.is_err());
}

#[test]
fn detect_three_node_cycle() {
    let mut lf = Lockfile::new();
    lf.add_crate(LockedCrate {
        name: "a".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Path {
            path: "/a".to_string(),
        },
        dependencies: {
            let mut m = BTreeMap::new();
            m.insert("b".to_string(), "1.0.0".to_string());
            m
        },
    });
    lf.add_crate(LockedCrate {
        name: "b".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Path {
            path: "/b".to_string(),
        },
        dependencies: {
            let mut m = BTreeMap::new();
            m.insert("c".to_string(), "1.0.0".to_string());
            m
        },
    });
    lf.add_crate(LockedCrate {
        name: "c".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Path {
            path: "/c".to_string(),
        },
        dependencies: {
            let mut m = BTreeMap::new();
            m.insert("a".to_string(), "1.0.0".to_string());
            m
        },
    });

    let resolver = DependencyResolver::new_no_index();
    let result = resolver.detect_cycles(&lf);
    assert!(result.is_err());
}

/// Tier 3.3: when no registry client is configured and a dependency is not in
/// the local index, the error must be actionable (hint about the `registry`
/// feature / local index) rather than a bare `DependencyNotFound`.
#[test]
fn registry_disabled_gives_actionable_error() {
    let dir = TempDir::new().expect("temp dir");
    // Empty index, no registry client (DependencyResolver::new has None).
    let index = CrateIndex::new();
    let mut deps = BTreeMap::new();
    deps.insert(
        "remote-crate".to_string(),
        Dependency::Simple("1.0".to_string()),
    );
    let config = make_config_with_deps("main", deps, BTreeMap::new());

    let resolver = DependencyResolver::new(index);
    let err = resolver.resolve(&config, dir.path()).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("remote-crate"),
        "error should name the missing dep: {msg}"
    );
    assert!(
        msg.contains("--features registry") || msg.contains("local index"),
        "error should hint at the registry feature / local index: {msg}"
    );
}

// Plan §23.4: lockfile validation against the manifest must report a conflict
// when a manifest-pinned dependency is missing or version-mismatched.
#[test]
fn validate_lockfile_detects_missing_and_mismatched() {
    use crate::lockfile::{LockConflict, Lockfile};

    // Manifest pins exact `serde = "1.0.0"` and `log = "0.4.0"`.
    let mut deps = BTreeMap::new();
    deps.insert("serde".to_string(), Dependency::Simple("1.0.0".to_string()));
    deps.insert("log".to_string(), Dependency::Simple("0.4.0".to_string()));
    let manifest = make_config_with_deps("main", deps, BTreeMap::new());

    // Lockfile has serde@1.0.0 (matches) but log under-locked at 0.3.0.
    let mut lockfile = Lockfile::new();
    lockfile.add_crate(LockedCrate {
        name: "serde".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Registry {
            url: "https://example.com".to_string(),
            checksum: "abc".to_string(),
        },
        dependencies: BTreeMap::new(),
    });
    // log is under-locked at 0.3.0 while the manifest wants 0.4.
    lockfile.add_crate(LockedCrate {
        name: "log".to_string(),
        version: "0.3.0".to_string(),
        source: CrateSource::Registry {
            url: "https://example.com".to_string(),
            checksum: "def".to_string(),
        },
        dependencies: BTreeMap::new(),
    });

    let conflicts = lockfile.validate_against_manifest(&manifest);
    // serde@1.0.0 satisfies the "1.0" pin → OK. log is mismatched.
    assert_eq!(
        conflicts.len(),
        1,
        "only log should conflict: {conflicts:?}"
    );
    assert!(matches!(
        &conflicts[0],
        LockConflict::VersionMismatch { name, .. } if name == "log"
    ));

    // A fully consistent lockfile yields no conflicts.
    let mut good = Lockfile::new();
    good.add_crate(LockedCrate {
        name: "serde".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Registry {
            url: "https://example.com".to_string(),
            checksum: "abc".to_string(),
        },
        dependencies: BTreeMap::new(),
    });
    good.add_crate(LockedCrate {
        name: "log".to_string(),
        version: "0.4.0".to_string(),
        source: CrateSource::Registry {
            url: "https://example.com".to_string(),
            checksum: "def".to_string(),
        },
        dependencies: BTreeMap::new(),
    });
    assert!(
        good.validate_against_manifest(&manifest).is_empty(),
        "consistent lockfile must validate clean"
    );

    // A manifest dep with no locked entry at all is a Missing conflict.
    let mut partial = Lockfile::new();
    partial.add_crate(LockedCrate {
        name: "serde".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Registry {
            url: "https://example.com".to_string(),
            checksum: "abc".to_string(),
        },
        dependencies: BTreeMap::new(),
    });
    let partial_conflicts = partial.validate_against_manifest(&manifest);
    assert_eq!(partial_conflicts.len(), 1);
    assert!(matches!(
        &partial_conflicts[0],
        LockConflict::Missing { name, .. } if name == "log"
    ));
}
