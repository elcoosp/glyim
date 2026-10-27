//! Tests for dependency resolution — S12-T01.

use crate::config::{Dependency, DependencyDetail, GlyipToml, PackageConfig};
use crate::dep::{CrateIndex, DependencyResolver, IndexDependency, IndexEntry, RegistryClient};
use crate::lockfile::CrateSource;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A mock registry client that returns pre-canned responses.
#[derive(Debug, Clone)]
struct MockRegistryClient {
    entries: HashMap<String, IndexEntry>,
    download_log: std::cell::RefCell<Vec<(String, String, PathBuf)>>,
}

impl MockRegistryClient {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            download_log: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn add_entry(&mut self, entry: IndexEntry) {
        self.entries.insert(entry.name.clone(), entry);
    }

    fn download_count(&self) -> usize {
        self.download_log.borrow().len()
    }
}

impl RegistryClient for MockRegistryClient {
    fn fetch_index(&self, name: &str) -> crate::error::GlyipResult<IndexEntry> {
        self.entries.get(name).cloned().ok_or_else(|| {
            crate::error::GlyipError::DependencyNotFound {
                name: name.to_string(),
                version: None,
            }
        })
    }

    fn download_crate(
        &self,
        name: &str,
        version: &str,
        expected_checksum: Option<&str>,
        dest: &Path,
    ) -> crate::error::GlyipResult<PathBuf> {
        let _ = expected_checksum;
        let extract_dir = dest.join(format!("{}-{}", name, version));
        let src_dir = extract_dir.join("src");
        std::fs::create_dir_all(&src_dir)?;
        std::fs::write(src_dir.join("lib.g"), "// mock crate\n")?;
        self.download_log.borrow_mut().push((
            name.to_string(),
            version.to_string(),
            dest.to_path_buf(),
        ));
        Ok(extract_dir)
    }
}

fn make_config_with_dep(name: &str, version: &str) -> GlyipToml {
    let mut deps = BTreeMap::new();
    deps.insert(name.to_string(), Dependency::Simple(version.to_string()));
    GlyipToml {
        package: PackageConfig {
            name: "test-project".to_string(),
            version: "0.1.0".to_string(),
            edition: "2024".to_string(),
            authors: Vec::new(),
            description: None,
            bin: None,
            lib: None,
        },
        dependencies: deps,
        dev_dependencies: BTreeMap::new(),
    }
}

fn make_config_with_path_dep(name: &str, path: &str) -> GlyipToml {
    let mut deps = BTreeMap::new();
    deps.insert(
        name.to_string(),
        Dependency::Detailed(DependencyDetail {
            version: None,
            path: Some(PathBuf::from(path)),
            git: None,
            branch: None,
            tag: None,
            rev: None,
        }),
    );
    GlyipToml {
        package: PackageConfig {
            name: "test-project".to_string(),
            version: "0.1.0".to_string(),
            edition: "2024".to_string(),
            authors: Vec::new(),
            description: None,
            bin: None,
            lib: None,
        },
        dependencies: deps,
        dev_dependencies: BTreeMap::new(),
    }
}

#[test]
fn resolve_transitive_dependencies() {
    // Tier 3.1: a crate's own dependencies must be resolved transitively,
    // even when the root project never mentions them directly.
    let mut index = CrateIndex::new();

    // `a` depends on `b` (version 0.5.0).
    index.insert(IndexEntry {
        name: "a".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: HashMap::new(),
        dependencies: {
            let mut m = HashMap::new();
            m.insert(
                "1.0.0".to_string(),
                vec![IndexDependency {
                    name: "b".to_string(),
                    version_req: Some("0.5".to_string()),
                }],
            );
            m
        },
    });

    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "b".to_string(),
        versions: vec!["0.5.0".to_string()],
        checksums: HashMap::new(),
    });

    let resolver = DependencyResolver::new(index);
    // Root only depends on `a`.
    let config = make_config_with_dep("a", "1.0");
    let dir = TempDir::new().unwrap();
    let lockfile = resolver.resolve(&config, dir.path()).unwrap();

    // `a` resolved...
    assert!(lockfile.get_crate("a", "1.0.0").is_some());
    // ...and its transitive dep `b` was pulled in automatically.
    assert!(
        lockfile.get_crate("b", "0.5.0").is_some(),
        "transitive dependency `b` should be in the lockfile even though the root never mentions it"
    );
    // `a`'s own lockfile entry records `b` as a dependency (storing the
    // requested version requirement, as produced by the index metadata).
    let a = lockfile.get_crate("a", "1.0.0").unwrap();
    assert_eq!(a.dependencies.get("b"), Some(&"0.5".to_string()));
}

#[test]
fn resolve_from_local_index() {
    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "foo".to_string(),
        versions: vec!["1.0.0".to_string(), "0.9.0".to_string()],
        checksums: {
            let mut m = HashMap::new();
            m.insert("1.0.0".to_string(), "abc123".to_string());
            m
        },
    });

    let resolver = DependencyResolver::new(index);
    let config = make_config_with_dep("foo", "1.0");
    let dir = TempDir::new().unwrap();
    let lockfile = resolver.resolve(&config, dir.path()).unwrap();

    assert!(lockfile.get_crate("foo", "1.0.0").is_some());
    let locked = lockfile.get_crate("foo", "1.0.0").unwrap();
    assert_eq!(locked.version, "1.0.0");
    match &locked.source {
        CrateSource::Registry { checksum, .. } => assert_eq!(checksum, "abc123"),
        _ => panic!("expected Registry source"),
    }
}

#[test]
fn resolve_from_registry_fetch() {
    // S12-T01: glyip build fetches dependency from registry.
    let mut mock = MockRegistryClient::new();
    mock.add_entry(IndexEntry {
        dependencies: Default::default(),
        name: "bar".to_string(),
        versions: vec!["2.1.0".to_string()],
        checksums: {
            let mut m = HashMap::new();
            m.insert("2.1.0".to_string(), "def456".to_string());
            m
        },
    });

    let resolver = DependencyResolver::new_no_index().with_registry_client(Box::new(mock));
    let config = make_config_with_dep("bar", "2");
    let dir = TempDir::new().unwrap();
    let lockfile = resolver.resolve(&config, dir.path()).unwrap();

    assert!(lockfile.get_crate("bar", "2.1.0").is_some());
    let locked = lockfile.get_crate("bar", "2.1.0").unwrap();
    assert_eq!(locked.version, "2.1.0");
    match &locked.source {
        CrateSource::Registry { checksum, .. } => assert_eq!(checksum, "def456"),
        _ => panic!("expected Registry source"),
    }
}

#[test]
fn registry_fallback_on_missing_index_entry() {
    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        dependencies: Default::default(),
        name: "foo".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: HashMap::new(),
    });

    let mut mock = MockRegistryClient::new();
    mock.add_entry(IndexEntry {
        dependencies: Default::default(),
        name: "bar".to_string(),
        versions: vec!["3.0.0".to_string()],
        checksums: HashMap::new(),
    });

    let resolver = DependencyResolver::new(index).with_registry_client(Box::new(mock));

    let mut deps = BTreeMap::new();
    deps.insert("foo".to_string(), Dependency::Simple("1.0".to_string()));
    deps.insert("bar".to_string(), Dependency::Simple("3".to_string()));

    let config = GlyipToml {
        package: PackageConfig {
            name: "test-project".to_string(),
            version: "0.1.0".to_string(),
            edition: "2024".to_string(),
            authors: Vec::new(),
            description: None,
            bin: None,
            lib: None,
        },
        dependencies: deps,
        dev_dependencies: BTreeMap::new(),
    };

    let dir = TempDir::new().unwrap();
    let lockfile = resolver.resolve(&config, dir.path()).unwrap();

    assert!(lockfile.get_crate("foo", "1.0.0").is_some());
    assert!(lockfile.get_crate("bar", "3.0.0").is_some());
}

#[test]
fn resolve_path_dependency() {
    let dir = TempDir::new().unwrap();
    let sub_dir = dir.path().join("sub-crate");
    std::fs::create_dir_all(sub_dir.join("src")).unwrap();
    std::fs::write(
        sub_dir.join("Glyip.toml"),
        "[package]\nname = \"sub-crate\"\nversion = \"0.2.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let resolver = DependencyResolver::new_no_index();
    let config = make_config_with_path_dep("sub-crate", sub_dir.to_str().unwrap());
    let lockfile = resolver.resolve(&config, dir.path()).unwrap();

    assert!(lockfile.get_crate("sub-crate", "0.2.0").is_some());
    let locked = lockfile.get_crate("sub-crate", "0.2.0").unwrap();
    match &locked.source {
        CrateSource::Path { path } => assert!(path.contains("sub-crate")),
        _ => panic!("expected Path source"),
    }
}

#[test]
fn detect_dependency_cycle() {
    let index = CrateIndex::new();
    let resolver = DependencyResolver::new(index);

    let mut lockfile = crate::lockfile::Lockfile::new();
    let mut a_deps = BTreeMap::new();
    a_deps.insert("b".to_string(), "1.0.0".to_string());
    lockfile.add_crate(crate::lockfile::LockedCrate {
        name: "a".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Registry {
            url: "https://index.glyim.dev".to_string(),
            checksum: String::new(),
        },
        dependencies: a_deps,
    });
    let mut b_deps = BTreeMap::new();
    b_deps.insert("a".to_string(), "1.0.0".to_string());
    lockfile.add_crate(crate::lockfile::LockedCrate {
        name: "b".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Registry {
            url: "https://index.glyim.dev".to_string(),
            checksum: String::new(),
        },
        dependencies: b_deps,
    });

    let result = resolver.detect_cycles(&lockfile);
    assert!(result.is_err());
    match result.unwrap_err() {
        crate::error::GlyipError::DependencyCycle(cycle) => {
            assert!(cycle.contains(&"a".to_string()) || cycle.contains(&"b".to_string()));
        }
        other => panic!("expected DependencyCycle, got {:?}", other),
    }
}

#[test]
fn download_crate_with_registry() {
    let mut mock = MockRegistryClient::new();
    mock.add_entry(IndexEntry {
        dependencies: Default::default(),
        name: "qux".to_string(),
        versions: vec!["1.0.0".to_string()],
        checksums: HashMap::new(),
    });

    let resolver = DependencyResolver::new_no_index().with_registry_client(Box::new(mock));

    let dir = TempDir::new().unwrap();
    let locked = crate::lockfile::LockedCrate {
        name: "qux".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Registry {
            url: "https://index.glyim.dev".to_string(),
            checksum: String::new(),
        },
        dependencies: BTreeMap::new(),
    };

    let result = resolver.download_crate(&locked, dir.path());
    assert!(result.is_ok());
    let extracted = result.unwrap();
    assert!(extracted.exists());
}

#[test]
fn download_crate_without_registry_fails() {
    let resolver = DependencyResolver::new_no_index();
    let dir = TempDir::new().unwrap();
    let locked = crate::lockfile::LockedCrate {
        name: "nope".to_string(),
        version: "1.0.0".to_string(),
        source: CrateSource::Registry {
            url: "https://index.glyim.dev".to_string(),
            checksum: String::new(),
        },
        dependencies: BTreeMap::new(),
    };

    let result = resolver.download_crate(&locked, dir.path());
    assert!(result.is_err());
}

#[test]
fn dependency_not_found_without_registry() {
    let resolver = DependencyResolver::new_no_index();
    let config = make_config_with_dep("nonexistent", "1.0");
    let dir = TempDir::new().unwrap();

    let result = resolver.resolve(&config, dir.path());
    assert!(result.is_err());
    match result.unwrap_err() {
        crate::error::GlyipError::DependencyNotFound { name, .. } => {
            assert_eq!(name, "nonexistent");
        }
        other => panic!("expected DependencyNotFound, got {:?}", other),
    }
}

#[test]
fn mock_registry_download_count() {
    let mut mock = MockRegistryClient::new();
    mock.add_entry(IndexEntry {
        dependencies: Default::default(),
        name: "counted".to_string(),
        versions: vec!["0.1.0".to_string()],
        checksums: HashMap::new(),
    });
    assert_eq!(mock.download_count(), 0);

    let dir = TempDir::new().unwrap();
    mock.download_crate("counted", "0.1.0", None, dir.path()).unwrap();
    assert_eq!(mock.download_count(), 1);


}

#[test]
fn path_dep_stored_relative_to_project_dir() {
    // P2-F1: a `Glyip.lock` checked into a repo must not embed absolute
    // filesystem paths from whatever machine generated it. When a path
    // dependency lives inside the project tree, the stored form is the
    // path *relative to the project directory*.
    let tmp = std::env::temp_dir().join(format!(
        "glyip_p2f1_relpath_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let project_dir = tmp.join("myproject");
    let dep_dir = project_dir.join("libs").join("util");
    std::fs::create_dir_all(&dep_dir).unwrap();

    // Dep crate's manifest.
    let dep_toml = "[package]\nname = \"util\"\nversion = \"0.1.0\"\n";
    std::fs::write(dep_dir.join("Glyip.toml"), dep_toml).unwrap();

    // Project manifest depending on it via a relative path.
    let project_toml = "[package]\nname = \"myproject\"\nversion = \"0.1.0\"\n\
                        [dependencies]\nutil = { path = \"libs/util\" }\n";
    std::fs::write(project_dir.join("Glyip.toml"), project_toml).unwrap();

    let cfg = GlyipToml::read_from_dir(&project_dir).unwrap();
    let resolver = DependencyResolver::new_no_index();
    let lock = resolver.resolve(&cfg, &project_dir).unwrap();

    let util = lock
        .get_crate("util", "0.1.0")
        .expect("util must appear in lockfile");
    match &util.source {
        CrateSource::Path { path } => {
            let p = std::path::Path::new(path);
            assert!(
                p.is_relative(),
                "P2-F1: stored path should be relative to project_dir, got {path:?} \
                 (absolute would break lockfile portability across machines)"
            );
            assert_eq!(
                path, "libs/util",
                "expected exactly the project-relative form; got {path:?}"
            );
        }
        other => panic!("expected CrateSource::Path, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&tmp);
}


/// P2-F2: a registry client that reports a non-default `base_url_display`.
/// The `fetch_index` and `download_crate` impls are `unimplemented!()`
/// because the test never calls them — the resolver's lockfile-write path
/// is reached via a pre-populated local `CrateIndex`, and only
/// `base_url_display` matters for provenance.
#[derive(Debug)]
struct CustomUrlRegistryClient {
    url: String,
}

impl RegistryClient for CustomUrlRegistryClient {
    fn base_url_display(&self) -> String {
        self.url.clone()
    }

    fn fetch_index(&self, _name: &str) -> crate::error::GlyipResult<IndexEntry> {
        unimplemented!("test never fetches from the registry")
    }

    fn download_crate(
        &self,
        _name: &str,
        _version: &str,
        _expected_checksum: Option<&str>,
        _dest: &Path,
    ) -> crate::error::GlyipResult<PathBuf> {
        unimplemented!("test never downloads")
    }
}

#[test]
fn custom_registry_url_recorded_in_lockfile() {
    // P2-F2: `CrateSource::Registry { url }` must record the *actual*
    // registry the crate was resolved against, not a hardcoded public-index
    // URL. Before this fix, a user pointing glyip at a corporate mirror got
    // a lockfile claiming every dep came from `https://index.glyim.dev`.
    let custom = "https://internal-mirror.example/glyim";

    let mut index = CrateIndex::new();
    index.insert(IndexEntry {
        name: "widget".to_string(),
        versions: vec!["1.0.0".to_string()],
        dependencies: Default::default(),
        checksums: HashMap::new(),
    });

    let resolver = DependencyResolver::new(index).with_registry_client(Box::new(
        CustomUrlRegistryClient {
            url: custom.to_string(),
        },
    ));

    let project_dir = TempDir::new().unwrap();
    let config = GlyipToml {
        package: PackageConfig {
            name: "root".to_string(),
            version: "0.1.0".to_string(),
            edition: "2024".to_string(),
            authors: Vec::new(),
            description: None,
            bin: None,
            lib: None,
        },
        dependencies: {
            let mut m = BTreeMap::new();
            m.insert(
                "widget".to_string(),
                Dependency::Simple("1.0.0".to_string()),
            );
            m
        },
        dev_dependencies: BTreeMap::new(),
    };

    let lock = resolver.resolve(&config, project_dir.path()).unwrap();

    let widget = lock
        .get_crate("widget", "1.0.0")
        .expect("widget must be in lockfile");

    match &widget.source {
        CrateSource::Registry { url, .. } => {
            assert_eq!(
                url, custom,
                "P2-F2: lockfile must record the actual registry URL, \
                 not the hardcoded public index"
            );
            assert_ne!(
                url, "https://index.glyim.dev",
                "P2-F2: the public-index URL must not appear when a \
                 custom registry client was attached"
            );
        }
        other => panic!("expected CrateSource::Registry, got {other:?}"),
    }
}
