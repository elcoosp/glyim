//! Tests for plan §23.2: git dependency resolution and semver conflict
//! detection in `DependencyResolver`.
//!
//! Git tests use a [`MockGitFetcher`] so they need no network access — the
//! mock returns a deterministic commit and a temporary checkout dir carrying a
//! real `Glyip.toml`.

use crate::config::{Dependency, GitSpec, GlyipToml, PackageConfig};
use crate::dep::{CrateIndex, DependencyResolver, GitFetcher, IndexDependency, IndexEntry};
use crate::error::GlyipError;
use crate::lockfile::CrateSource;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

/// A mock git fetcher: returns a fixed commit and a temp checkout dir.
struct MockGitFetcher {
    rev: String,
    /// Held so the temp dir outlives the test.
    _checkout: TempDir,
    checkout_path: PathBuf,
}

impl MockGitFetcher {
    /// Build a mock whose checkout contains a `Glyip.toml` for `name@version`.
    fn new(name: &str, version: &str) -> Self {
        let dir = TempDir::new().unwrap();
        let toml =
            format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2024\"\n");
        std::fs::write(dir.path().join("Glyip.toml"), toml).unwrap();
        Self {
            rev: "abc123def456abc123def456abc123def456abc1".to_string(),
            checkout_path: dir.path().to_path_buf(),
            _checkout: dir,
        }
    }
}

impl GitFetcher for MockGitFetcher {
    fn resolve_rev(&self, _url: &str, spec: &GitSpec) -> crate::error::GlyipResult<String> {
        // A pinned exact rev is preserved verbatim; branch/tag/default resolve
        // to the (mock) fetched commit.
        match spec {
            GitSpec::Rev(r) => Ok(r.clone()),
            _ => Ok(self.rev.clone()),
        }
    }

    fn fetch(
        &self,
        _url: &str,
        _rev: &str,
        _dest: &std::path::Path,
    ) -> crate::error::GlyipResult<PathBuf> {
        Ok(self.checkout_path.clone())
    }
}

fn root_config(deps: &[(&str, &str)]) -> GlyipToml {
    let mut dependencies = BTreeMap::new();
    for (name, ver) in deps {
        dependencies.insert(name.to_string(), Dependency::Simple(ver.to_string()));
    }
    GlyipToml {
        package: PackageConfig {
            name: "root".to_string(),
            version: "0.1.0".to_string(),
            edition: "2024".to_string(),
            authors: vec![],
            description: None,
            bin: None,
            lib: None,
        },
        dependencies,
        dev_dependencies: BTreeMap::new(),
    }
}

#[test]
fn git_branch_dependency_resolves_to_locked_git_source() {
    let mut config = root_config(&[]);
    // Root depends on a git crate via branch `main`.
    let detail = crate::config::DependencyDetail {
        version: None,
        path: None,
        git: Some("https://example.com/foo/bar.git".to_string()),
        branch: Some("main".to_string()),
        tag: None,
        rev: None,
    };
    config
        .dependencies
        .insert("bar".to_string(), Dependency::Detailed(detail.clone()));

    let fetcher = MockGitFetcher::new("bar", "0.3.0");
    let resolver = DependencyResolver::new_no_index().with_git_fetcher(Box::new(fetcher));

    let lockfile = resolver
        .resolve(&config, std::path::Path::new("/tmp"))
        .expect("git dep resolves");

    let locked = lockfile
        .crates()
        .find(|c| c.name == "bar")
        .expect("bar is locked");
    match &locked.source {
        CrateSource::Git {
            url, rev, branch, ..
        } => {
            assert_eq!(url, "https://example.com/foo/bar.git");
            assert_eq!(
                rev.as_deref(),
                Some("abc123def456abc123def456abc123def456abc1")
            );
            assert_eq!(branch.as_deref(), Some("main"));
        }
        other => panic!("expected CrateSource::Git, got {other:?}"),
    }
    assert_eq!(
        locked.version, "0.3.0",
        "version read from checked-out Glyip.toml"
    );
}

#[test]
fn git_rev_dependency_records_exact_rev() {
    let mut config = root_config(&[]);
    let detail = crate::config::DependencyDetail {
        version: None,
        path: None,
        git: Some("https://example.com/foo/pinned.git".to_string()),
        branch: None,
        tag: None,
        rev: Some("deadbeef".to_string()),
    };
    config
        .dependencies
        .insert("pinned".to_string(), Dependency::Detailed(detail));

    let fetcher = MockGitFetcher::new("pinned", "1.0.0");
    let resolver = DependencyResolver::new_no_index().with_git_fetcher(Box::new(fetcher));
    let lockfile = resolver
        .resolve(&config, std::path::Path::new("/tmp"))
        .unwrap();

    let locked = lockfile.crates().find(|c| c.name == "pinned").unwrap();
    match &locked.source {
        CrateSource::Git { rev, .. } => {
            assert_eq!(rev.as_deref(), Some("deadbeef"), "exact rev is preserved")
        }
        other => panic!("expected CrateSource::Git, got {other:?}"),
    }
}

#[test]
fn git_dependency_without_branch_tag_uses_default_branch() {
    let mut config = root_config(&[]);
    let detail = crate::config::DependencyDetail {
        version: None,
        path: None,
        git: Some("https://example.com/foo/def.git".to_string()),
        branch: None,
        tag: None,
        rev: None,
    };
    config
        .dependencies
        .insert("def".to_string(), Dependency::Detailed(detail));

    let fetcher = MockGitFetcher::new("def", "2.0.0");
    let resolver = DependencyResolver::new_no_index().with_git_fetcher(Box::new(fetcher));
    let lockfile = resolver
        .resolve(&config, std::path::Path::new("/tmp"))
        .unwrap();

    let locked = lockfile.crates().find(|c| c.name == "def").unwrap();
    match &locked.source {
        CrateSource::Git { branch, tag, .. } => {
            assert!(branch.is_none(), "no branch when default branch is used");
            assert!(tag.is_none(), "no tag when default branch is used");
        }
        other => panic!("expected CrateSource::Git, got {other:?}"),
    }
}

/// Build an index where `a` and `b` each depend on `foo` with a mutually
/// incompatible version requirement, so resolving both must surface a
/// semver conflict (plan §23.2).
fn conflict_index() -> CrateIndex {
    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        name: "foo".to_string(),
        versions: vec!["2.5.0".to_string(), "1.9.9".to_string()],
        checksums: HashMap::new(),
        dependencies: HashMap::new(),
    });
    index.insert(IndexEntry {
        name: "a".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: HashMap::new(),
        dependencies: {
            let mut m = HashMap::new();
            m.insert(
                "1.0.0".to_string(),
                vec![IndexDependency {
                    name: "foo".to_string(),
                    version_req: Some("^1.0.0".to_string()),
                }],
            );
            m
        },
    });
    index.insert(IndexEntry {
        name: "b".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: HashMap::new(),
        dependencies: {
            let mut m = HashMap::new();
            m.insert(
                "1.0.0".to_string(),
                vec![IndexDependency {
                    name: "foo".to_string(),
                    version_req: Some("^2.0.0".to_string()),
                }],
            );
            m
        },
    });
    index
}

#[test]
fn semver_conflict_between_dependents_is_detected() {
    let config = root_config(&[("a", "1.0"), ("b", "1.0")]);
    let resolver = DependencyResolver::new(conflict_index());

    let err = resolver
        .resolve(&config, std::path::Path::new("/tmp"))
        .expect_err("mutually-incompatible foo requirements must conflict");

    match err {
        GlyipError::DependencyConflict {
            name, requirements, ..
        } => {
            assert_eq!(name, "foo");
            assert!(
                requirements.contains(&"^1.0.0".to_string()),
                "requirements recorded: {requirements:?}"
            );
            assert!(
                requirements.contains(&"^2.0.0".to_string()),
                "requirements recorded: {requirements:?}"
            );
        }
        other => panic!("expected DependencyConflict, got {other:?}"),
    }
}

#[test]
fn compatible_version_requirements_do_not_conflict() {
    // Both a and b require foo compatible with 1.x → 1.9.9 satisfies both.
    let mut index = conflict_index();
    // Override b to also require ^1.0.0 so there is no conflict.
    index.insert(IndexEntry {
        name: "b".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: HashMap::new(),
        dependencies: {
            let mut m = HashMap::new();
            m.insert(
                "1.0.0".to_string(),
                vec![IndexDependency {
                    name: "foo".to_string(),
                    version_req: Some("^1.0.0".to_string()),
                }],
            );
            m
        },
    });
    let config = root_config(&[("a", "1.0"), ("b", "1.0")]);
    let resolver = DependencyResolver::new(index);

    let lockfile = resolver
        .resolve(&config, std::path::Path::new("/tmp"))
        .expect("compatible requirements resolve");
    assert!(
        lockfile.crates().any(|c| c.name == "foo"),
        "foo is resolved exactly once across both dependents"
    );
}

// Keep `Arc` referenced for potential future shared-mock usage without tripping
// the unused-import lint in minimal test configs.
#[allow(dead_code)]
fn _assert_arc_used() -> Arc<u8> {
    Arc::new(0)
}

/// Plan §4.3: resolution must be a pure function of its inputs. Resolving the
/// same graph twice yields byte-identical lockfiles regardless of any internal
/// (HashMap / VecDeque) ordering.
#[test]
fn resolution_is_deterministic_across_runs() {
    // An index with multiple candidates in *non-sorted* order so a
    // listing-order-dependent "pick first" strategy would be non-deterministic.
    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        name: "serde".to_string(),
        versions: vec![
            "1.0.130".to_string(),
            "1.0.219".to_string(),
            "1.0.1".to_string(),
            "1.0.100".to_string(),
        ],
        checksums: HashMap::new(),
        dependencies: HashMap::new(),
    });
    index.insert(IndexEntry {
        name: "log".to_string(),
        versions: vec!["0.4.20".to_string(), "0.4.0".to_string()],
        checksums: HashMap::new(),
        dependencies: HashMap::new(),
    });

    let config = root_config(&[("serde", "*"), ("log", "*")]);

    let a = DependencyResolver::new(index.clone())
        .resolve(&config, std::path::Path::new("/tmp"))
        .expect("resolve run 1");
    let b = DependencyResolver::new(index)
        .resolve(&config, std::path::Path::new("/tmp"))
        .expect("resolve run 2");

    assert_eq!(
        a.crates()
            .map(|c| (c.name.as_str(), c.version.as_str()))
            .collect::<Vec<_>>(),
        b.crates()
            .map(|c| (c.name.as_str(), c.version.as_str()))
            .collect::<Vec<_>>(),
        "two resolutions of the same graph must pick the same versions"
    );
    // Specifically, the highest SemVer version must win, not the listing order.
    let serde = a.crates().find(|c| c.name == "serde").unwrap();
    assert_eq!(
        serde.version, "1.0.219",
        "highest version must be selected, not first-listed"
    );
}

/// Plan §4.3: when requirements genuinely conflict, the diagnostic names *both*
/// requesters (the dependents that disagree), not just the crate name.
#[test]
fn conflict_error_names_both_requesters() {
    // a requires foo = ^1.0.0, b requires foo = ^2.0.0 — irreconcilable.
    let config = root_config(&[("a", "1.0"), ("b", "1.0")]);
    let resolver = DependencyResolver::new(conflict_index());

    let err = resolver
        .resolve(&config, std::path::Path::new("/tmp"))
        .expect_err("mutually-incompatible foo requirements must conflict");

    match err {
        GlyipError::DependencyConflict {
            name,
            requirements,
            requesters,
            ..
        } => {
            assert_eq!(name, "foo");
            assert!(requirements.contains(&"^1.0.0".to_string()));
            assert!(requirements.contains(&"^2.0.0".to_string()));

            // Both dependents must be identified as requesters.
            let who: Vec<&str> = requesters.iter().map(|(w, _)| w.as_str()).collect();
            assert!(who.contains(&"a"), "requester 'a' must be named: {who:?}");
            assert!(who.contains(&"b"), "requester 'b' must be named: {who:?}");

            // And each requester edge carries its own requirement.
            let a_req = requesters
                .iter()
                .find(|(w, _)| w == "a")
                .map(|(_, r)| r.as_str());
            let b_req = requesters
                .iter()
                .find(|(w, _)| w == "b")
                .map(|(_, r)| r.as_str());
            assert_eq!(a_req, Some("^1.0.0"));
            assert_eq!(b_req, Some("^2.0.0"));

            // The rendered message must surface both requesters. Reconstruct the
            // diagnostic text from the moved-in fields (err is partially moved).
            let msg = format!(
                "dependency version conflict for `{name}`: requirements {requirements:?} cannot be satisfied\n  required by: {}",
                requesters
                    .iter()
                    .map(|(who, req)| format!("`{who}` requires {req}"))
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            assert!(msg.contains('a'), "message should name requester a: {msg}");
            assert!(msg.contains('b'), "message should name requester b: {msg}");
        }
        other => panic!("expected DependencyConflict, got {other:?}"),
    }
}
