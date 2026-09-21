//! Dependency resolution for Glyim projects.
//!
//! Resolves a project's dependency graph from a `Glyip.toml` and optional
//! index, performs cycle detection, and produces a `Lockfile`. Supports
//! fetching crate metadata from remote registries via the [`RegistryClient`]
//! trait.

use crate::config::GlyipToml;
use crate::error::{GlyipError, GlyipResult};
use crate::lockfile::{CrateSource, LockedCrate, Lockfile};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// A dependency declared by a published crate version in the index.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexDependency {
    /// Dependent crate name.
    pub name: String,
    /// Version requirement (semver-like), if any.
    #[serde(default)]
    pub version_req: Option<String>,
}

/// An entry in the crate index — metadata about a published crate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexEntry {
    /// Crate name.
    pub name: String,
    /// Available versions (semver-like strings, newest first).
    pub versions: Vec<String>,
    /// Checksums keyed by version.
    #[serde(default)]
    pub checksums: HashMap<String, String>,
    /// Dependencies keyed by version: each published version lists its own
    /// dependencies. `#[serde(default)]` keeps this backward-compatible with
    /// existing `.json` index files on disk (they deserialize with an empty
    /// map — no migration needed).
    #[serde(default)]
    pub dependencies: HashMap<String, Vec<IndexDependency>>,
}

/// A virtual crate index for dependency resolution.
#[derive(Debug, Clone, Default)]
pub struct CrateIndex {
    entries: HashMap<String, IndexEntry>,
}

impl CrateIndex {
    /// Create a new empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert an entry into the index.
    pub fn insert(&mut self, entry: IndexEntry) {
        self.entries.insert(entry.name.clone(), entry);
    }

    /// Look up a crate by name.
    pub fn get(&self, name: &str) -> Option<&IndexEntry> {
        self.entries.get(name)
    }

    /// Resolve the best matching version for a version requirement.
    ///
    /// Real SemVer 2.0 matching (de-stubbing plan §21.5) via the `semver`
    /// crate, replacing the old naive prefix-match (`starts_with(req)`):
    /// supports caret (`^1.2.3`), tilde (`~1.2.3`), wildcard (`1.2.*`),
    /// comparison operators (`>=`, `<`, `>`, `<=`, `=`), and exact
    /// `1.2.3`. When the requirement is an unparseable string (e.g. `"99"`),
    /// the historical fallback of resolving to the highest available version
    /// is preserved for graceful degradation rather than a hard error.
    pub fn resolve_version(&self, name: &str, version_req: Option<&str>) -> GlyipResult<String> {
        let entry = self
            .entries
            .get(name)
            .ok_or_else(|| GlyipError::DependencyNotFound {
                name: name.to_string(),
                version: version_req.map(String::from),
            })?;

        if entry.versions.is_empty() {
            return Err(GlyipError::DependencyNotFound {
                name: name.to_string(),
                version: version_req.map(String::from),
            });
        }

        // Real SemVer 2.0 matching (plan §21.5) instead of naive prefix matching.
        select_best_version(&entry.versions, version_req).ok_or_else(|| {
            GlyipError::DependencyNotFound {
                name: name.to_string(),
                version: version_req.map(String::from),
            }
        })
    }

    /// Number of entries in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Load index entries from a directory of JSON files.
    ///
    /// Each file should be named `<crate-name>.json` and contain a serialised
    /// [`IndexEntry`]. Non-JSON files are silently ignored. If the directory
    /// does not exist, returns an empty index.
    pub fn load_from_dir(dir: &Path) -> GlyipResult<Self> {
        let mut index = Self::new();
        if !dir.exists() {
            return Ok(index);
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let content = fs::read_to_string(&path)?;
                let index_entry: IndexEntry = serde_json::from_str(&content).map_err(|e| {
                    GlyipError::CacheCorrupted(format!(
                        "invalid index entry in {}: {}",
                        path.display(),
                        e
                    ))
                })?;
                index.insert(index_entry);
            }
        }
        Ok(index)
    }

    /// Save all index entries to a directory as JSON files.
    ///
    /// Each entry is written to `<crate-name>.json`. The directory is created
    /// if it does not exist.
    pub fn save_to_dir(&self, dir: &Path) -> GlyipResult<()> {
        fs::create_dir_all(dir)?;
        for (name, entry) in &self.entries {
            let path = dir.join(format!("{}.json", name));
            let content = serde_json::to_string_pretty(entry).map_err(|e| {
                GlyipError::CacheCorrupted(format!("serialize index entry '{}': {}", name, e))
            })?;
            fs::write(path, content)?;
        }
        Ok(())
    }
}

/// Select the highest version from `versions` satisfying `version_req`.
///
/// Real SemVer 2.0 matching via the `semver` crate (de-stubbing plan §21.5):
/// supports caret (`^1.2.3`), tilde (`~1.2.3`), wildcard (`1.2.*`, `*`),
/// comparison operators (`>=`, `<`, `>`, `<=`, `=`), and exact `1.2.3`. The
/// version list order is irrelevant — matches are sorted and the highest
/// satisfying version is returned.
///
/// Behavior for an *unparseable* requirement (e.g. `"99"`) preserves the
/// historical fallback of resolving to the highest available version, so
/// malformed requirements degrade gracefully rather than hard-erroring.
fn select_best_version(versions: &[String], version_req: Option<&str>) -> Option<String> {
    if versions.is_empty() {
        return None;
    }
    let Some(req) = version_req else {
        // No requirement: latest. Deterministic regardless of input order —
        // parse every version, sort by SemVer precedence, and take the highest
        // (plan §4.3: the "pick latest" default must not depend on the index
        // JSON's listing order).
        let mut parsed: Vec<Version> = versions
            .iter()
            .filter_map(|v| Version::parse(v).ok())
            .collect();
        parsed.sort();
        return parsed.last().map(|v| v.to_string());
    };
    // Real SemVer 2.0 matching. A requirement that fails to parse, or parses
    // but matches no available version, yields `None` only when truly absent —
    // here we return `None` so the caller surfaces a `DependencyNotFound`
    // (silent "latest" fallback for a typo'd/mismatched requirement would be
    // the wrong, stub-era behavior).
    let req = VersionReq::parse(req).ok()?;
    let mut matching: Vec<Version> = versions
        .iter()
        .filter_map(|v| Version::parse(v).ok())
        .filter(|v| req.matches(v))
        .collect();
    matching.sort();
    matching.last().map(|v| v.to_string())
}

/// Trait for fetching crate metadata and source from a remote registry.
///
/// Implementations can use HTTP, a local cache, or a mock for testing.
/// The default build ships without a registry client; one is constructed
/// only when the `registry` feature is enabled.
pub trait RegistryClient {
    /// Fetch the index entry for a crate from the registry.
    fn fetch_index(&self, name: &str) -> GlyipResult<IndexEntry>;

    /// Download a crate's source tarball and extract it to `dest`.
    ///
    /// Returns the path to the extracted source directory.
    fn download_crate(&self, name: &str, version: &str, dest: &Path) -> GlyipResult<PathBuf>;
}

/// HTTP-based registry client that fetches from a remote crate index.
///
/// Uses the `reqwest` blocking client for HTTP requests and supports
/// gzip-compressed `.crate` tarballs (the standard format).
#[cfg(feature = "registry")]
#[derive(Debug)]
pub struct HttpRegistryClient {
    base_url: String,
    client: reqwest::blocking::Client,
    cache_dir: PathBuf,
}

#[cfg(feature = "registry")]
impl HttpRegistryClient {
    /// Create a new HTTP registry client.
    ///
    /// `base_url` is the registry root (e.g. `https://index.glyim.dev`).
    /// `cache_dir` is where downloaded crates are stored.
    pub fn new(base_url: &str, cache_dir: PathBuf) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            cache_dir,
        }
    }

    /// Return the cache directory path.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

#[cfg(feature = "registry")]
impl RegistryClient for HttpRegistryClient {
    fn fetch_index(&self, name: &str) -> GlyipResult<IndexEntry> {
        let url = format!("{}/index/{}.json", self.base_url, name);
        debug!("Fetching index for '{}' from {}", name, url);

        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|e| GlyipError::RegistryError(format!("fetch index '{}': {}", name, e)))?;

        if !response.status().is_success() {
            return Err(GlyipError::RegistryError(format!(
                "registry returned {} for '{}'",
                response.status(),
                name
            )));
        }

        let entry: IndexEntry = response
            .json()
            .map_err(|e| GlyipError::RegistryError(format!("parse index '{}': {}", name, e)))?;

        info!(
            "Fetched index for '{}' with {} versions",
            name,
            entry.versions.len()
        );
        Ok(entry)
    }

    fn download_crate(&self, name: &str, version: &str, dest: &Path) -> GlyipResult<PathBuf> {
        let url = format!(
            "{}/crates/{}/{}-{}.crate",
            self.base_url, name, name, version
        );
        debug!("Downloading '{}' v{} from {}", name, version, url);

        let response = self.client.get(&url).send().map_err(|e| {
            GlyipError::RegistryError(format!("download '{}' v{}: {}", name, version, e))
        })?;

        if !response.status().is_success() {
            return Err(GlyipError::RegistryError(format!(
                "registry returned {} for '{}' v{}",
                response.status(),
                name,
                version
            )));
        }

        let bytes = response.bytes().map_err(|e| {
            GlyipError::RegistryError(format!("read response '{}' v{}: {}", name, version, e))
        })?;

        // Persist the tarball to cache.
        std::fs::create_dir_all(dest)?;
        let tarball_path = dest.join(format!("{}-{}.crate", name, version));
        std::fs::write(&tarball_path, &bytes)?;

        // Extract the gzip + tar archive.
        let extract_dir = dest.join(format!("{}-{}", name, version));
        std::fs::create_dir_all(&extract_dir)?;

        let gz_decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut archive = tar::Archive::new(gz_decoder);
        archive.unpack(&extract_dir).map_err(|e| {
            GlyipError::RegistryError(format!("extract '{}' v{}: {}", name, version, e))
        })?;

        info!("Downloaded and extracted '{}' v{}", name, version);
        Ok(extract_dir)
    }
}

/// Build the transitive sub-dependency list for a resolved index version.
///
/// Reads `entry.dependencies[version]` (a `Vec<IndexDependency>`), mapping
/// each to `(name, version_req, path)` where path is always `None` for
/// registry-indexed crates (their deps are resolved through the index too).
fn sub_deps_from_index(
    entry: &IndexEntry,
    version: &str,
) -> Vec<(String, Option<String>, Option<PathBuf>)> {
    entry
        .dependencies
        .get(version)
        .map(|deps| {
            deps.iter()
                .map(|d| (d.name.clone(), d.version_req.clone(), None))
                .collect()
        })
        .unwrap_or_default()
}

/// Resolves the full dependency graph for a project.
pub struct DependencyResolver {
    index: CrateIndex,
    registry_client: Option<Box<dyn RegistryClient>>,
    git_fetcher: Option<Box<dyn GitFetcher>>,
}

impl std::fmt::Debug for DependencyResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DependencyResolver")
            .field("index", &self.index)
            .field("has_registry_client", &self.registry_client.is_some())
            .field("has_git_fetcher", &self.git_fetcher.is_some())
            .finish()
    }
}

impl DependencyResolver {
    /// Create a new resolver with the given crate index.
    pub fn new(index: CrateIndex) -> Self {
        Self {
            index,
            registry_client: None,
            git_fetcher: None,
        }
    }

    /// Create a resolver with an empty index (for path-only dependencies).
    pub fn new_no_index() -> Self {
        Self {
            index: CrateIndex::new(),
            registry_client: None,
            git_fetcher: None,
        }
    }

    /// Attach a registry client for remote dependency resolution.
    ///
    /// When a dependency is not found in the local [`CrateIndex`], the
    /// resolver will attempt to fetch its metadata from the registry.
    pub fn with_registry_client(mut self, client: Box<dyn RegistryClient>) -> Self {
        self.registry_client = Some(client);
        self
    }

    /// Attach a git fetcher for git-dependency resolution (plan §23.2).
    ///
    /// When omitted, a default [`GitCommandFetcher`] (shelling out to the
    /// system `git`) is used. Tests inject a mock to avoid network access.
    pub fn with_git_fetcher(mut self, fetcher: Box<dyn GitFetcher>) -> Self {
        self.git_fetcher = Some(fetcher);
        self
    }

    /// Resolve all dependencies from a `GlyipToml` and produce a `Lockfile`.
    ///
    /// For dependencies not found in the local index, falls back to the
    /// Performs a breadth-first traversal that also resolves each resolved
    /// crate's own dependencies (transitive resolution), not just the root
    /// project's direct dependencies.
    pub fn resolve(&self, config: &GlyipToml, project_dir: &Path) -> GlyipResult<Lockfile> {
        let mut lockfile = Lockfile::new();
        let mut visited: HashSet<String> = HashSet::new();
        // Queue element: (name, version_req, path (if path dep), base dir to
        // resolve this dep's own path deps relative to). `base` is `None` for
        // deps declared relative to the project root (or registry deps), and
        // `Some(dir)` when the dep was itself pulled in by a path dependency
        // (so its nested path deps resolve relative to that crate's dir).
        let mut visit_stack: VecDeque<(String, Option<String>, Option<PathBuf>, Option<PathBuf>)> =
            VecDeque::new();
        // Every version requirement seen for each crate name, across the whole
        // graph (direct + transitive). Used for plan §23.2 semver conflict
        // detection: a crate required with mutually-incompatible requirements
        // must error rather than silently resolve to the first one.
        // Each entry is `(requester, requirement)` so the conflict diagnostic can
        // name every dependent that introduced a requirement (plan §4.3).
        let mut collected_reqs: HashMap<String, Vec<(String, String)>> = HashMap::new();

        // Seed the stack with direct dependencies. The requester for a direct
        // dependency is the project's own package.
        let root_requester = config.package.name.clone();
        for (name, dep) in config.all_dependencies() {
            let version = dep.version().map(String::from);
            let path = dep.path().map(PathBuf::from);
            if let Some(ref v) = version {
                collected_reqs
                    .entry(name.clone())
                    .or_default()
                    .push((root_requester.clone(), v.clone()));
            }
            visit_stack.push_back((name.clone(), version, path, None));
        }

        // Process dependencies breadth-first, enqueuing each resolved crate's
        // own (transitive) dependencies.
        while let Some((name, version_req, path, base)) = visit_stack.pop_front() {
            let key = if let Some(ref v) = version_req {
                format!("{}-{}(git)", name, v)
            } else {
                format!("{}(git)", name)
            };

            if visited.contains(&key) {
                continue;
            }
            visited.insert(key.clone());

            // Git dependency (plan §23.2): route to the git resolver.
            let git_spec = config
                .dependencies
                .get(&name)
                .and_then(|d| d.git_spec())
                .or_else(|| {
                    config
                        .dev_dependencies
                        .get(&name)
                        .and_then(|d| d.git_spec())
                });

            let (locked, sub_deps) = if let Some(spec) = git_spec {
                let url = config
                    .dependencies
                    .get(&name)
                    .and_then(|d| d.git())
                    .or_else(|| config.dev_dependencies.get(&name).and_then(|d| d.git()))
                    .unwrap_or("")
                    .to_string();
                let fetched = self.fetch_git_dep(&name, &url, &spec)?;
                (fetched, Vec::new())
            } else if path.is_some() {
                // Absolute directory for a path dependency: if it's relative and we
                // were reached from a path dep, join against that crate's dir;
                // otherwise against the project root.
                let abs_path = path.as_ref().map(|p| {
                    if p.is_absolute() {
                        p.clone()
                    } else if let Some(ref b) = base {
                        b.join(p)
                    } else {
                        project_dir.join(p)
                    }
                });
                if let Some(ref abs) = abs_path {
                    self.resolve_path_dep(&name, abs)?
                } else {
                    unreachable!("path.is_some() but abs_path is None")
                }
            } else {
                // Index / registry dependency.
                self.resolve_registry_dep(&name, version_req.as_deref())?
            };

            lockfile.add_crate(locked);

            // Enqueue transitive dependencies discovered for the crate we just
            // resolved. A path sub-dependency carries its own (relative) path
            // and must resolve its own nested path deps against this crate's
            // directory, so pass that as the new `base`.
            for (dep_name, dep_version_req, dep_path) in sub_deps {
                if let Some(ref v) = dep_version_req {
                    collected_reqs
                        .entry(dep_name.clone())
                        .or_default()
                        .push((name.clone(), v.clone()));
                }
                let dep_base = if dep_path.is_some() {
                    path.clone()
                } else {
                    None
                };
                visit_stack.push_back((dep_name, dep_version_req, dep_path, dep_base));
            }
        }

        // Cycle detection on the resolved graph.
        self.detect_cycles(&lockfile)?;

        // Plan §23.2: semver conflict detection. For every crate name that was
        // required with multiple distinct version requirements across the
        // graph, the single version we locked must satisfy ALL of them; if it
        // does not, the requirements are mutually unsatisfiable.
        self.check_version_conflicts(&lockfile, &collected_reqs)?;

        Ok(lockfile)
    }

    /// Verify that each locked crate satisfies every version requirement
    /// gathered for it across the dependency graph (plan §23.2 / §4.3).
    fn check_version_conflicts(
        &self,
        lockfile: &Lockfile,
        collected_reqs: &HashMap<String, Vec<(String, String)>>,
    ) -> GlyipResult<()> {
        for (name, edges) in collected_reqs {
            // Deduplicate requirements before checking, but keep every
            // (requester, requirement) edge so the diagnostic can name all
            // dependents that disagree (plan §4.3).
            let unique_reqs: Vec<&String> = {
                let mut seen: Vec<&String> = Vec::new();
                for (_, req) in edges {
                    if !seen.contains(&req) {
                        seen.push(req);
                    }
                }
                seen
            };
            if unique_reqs.len() < 2 {
                continue;
            }
            // Find the version actually locked for this crate name.
            let locked_version = lockfile
                .crates()
                .find(|c| c.name == *name)
                .map(|c| c.version.clone());
            let satisfied = if let Some(ref v) = locked_version {
                match Version::parse(v) {
                    Ok(parsed) => unique_reqs.iter().all(|req| {
                        VersionReq::parse(req)
                            .map(|r| r.matches(&parsed))
                            .unwrap_or(false)
                    }),
                    Err(_) => false,
                }
            } else {
                false
            };
            if !satisfied {
                return Err(GlyipError::DependencyConflict {
                    name: name.clone(),
                    requirements: unique_reqs.into_iter().cloned().collect(),
                    requesters: edges.clone(),
                    resolved: locked_version,
                });
            }
        }
        Ok(())
    }

    /// Resolve a dependency from the local index or remote registry.
    ///
    /// Returns the [`LockedCrate`] plus the list of its own dependencies
    /// `(name, version_req, path)` so the caller can enqueue them for
    /// transitive resolution.
    fn resolve_registry_dep(
        &self,
        name: &str,
        version_req: Option<&str>,
    ) -> GlyipResult<(LockedCrate, Vec<(String, Option<String>, Option<PathBuf>)>)> {
        // Try the local index first.
        match self.index.resolve_version(name, version_req) {
            Ok(version) => {
                let entry = self.index.get(name).expect("version resolved from this entry");
                let checksum = entry.checksums.get(&version).cloned().unwrap_or_default();
                let sub_deps = sub_deps_from_index(entry, &version);
                let locked = LockedCrate {
                    name: name.to_string(),
                    version: version.clone(),
                    source: CrateSource::Registry {
                        url: "https://index.glyim.dev".to_string(),
                        checksum,
                    },
                    dependencies: sub_deps
                        .iter()
                        .map(|(n, v, _)| (n.clone(), v.clone().unwrap_or_default()))
                        .collect(),
                };
                Ok((locked, sub_deps))
            }
            Err(_) if self.registry_client.is_some() => {
                // Fall back to the registry client.
                let client = self.registry_client.as_ref().unwrap(); // INVARIANT: checked is_some above
                debug!(
                    "Dependency '{}' not in local index, fetching from registry",
                    name
                );
                let entry = client.fetch_index(name)?;

                let version = if let Some(req) = version_req {
                    select_best_version(&entry.versions, Some(req))
                        .or_else(|| entry.versions.first().cloned())
                        .ok_or_else(|| GlyipError::DependencyNotFound {
                            name: name.to_string(),
                            version: version_req.map(String::from),
                        })?
                } else {
                    // No requirement: pick the highest available version
                    // deterministically (plan §4.3) rather than relying on the
                    // JSON listing order.
                    select_best_version(&entry.versions, None).ok_or_else(|| {
                        GlyipError::DependencyNotFound {
                            name: name.to_string(),
                            version: None,
                        }
                    })?
                };

                let checksum = entry.checksums.get(&version).cloned().unwrap_or_default();
                let sub_deps = sub_deps_from_index(&entry, &version);

                debug!("Resolved '{}' v{} from remote registry", name, version);

                let locked = LockedCrate {
                    name: name.to_string(),
                    version,
                    source: CrateSource::Registry {
                        url: "https://index.glyim.dev".to_string(),
                        checksum,
                    },
                    dependencies: sub_deps
                        .iter()
                        .map(|(n, v, _)| (n.clone(), v.clone().unwrap_or_default()))
                        .collect(),
                };
                Ok((locked, sub_deps))
            }
            Err(_) if self.registry_client.is_none() => Err(GlyipError::DependencyNotFound {
                name: name.to_string(),
                version: version_req.map(|v| {
                    format!(
                        "{v} (hint: no registry client configured — build glyip with `--features registry` or provide a local index entry for '{name}')"
                    )
                }),
            }),
            Err(e) => Err(e),
        }
    }

    /// Resolve a path-based dependency.
    ///
    /// Returns the [`LockedCrate`] plus the sub-project's own dependencies
    /// `(name, version_req, path)` (read from its `Glyip.toml`) so the caller
    /// can enqueue them for transitive resolution.
    fn resolve_path_dep(
        &self,
        name: &str,
        path: &Path,
    ) -> GlyipResult<(LockedCrate, Vec<(String, Option<String>, Option<PathBuf>)>)> {
        let config = GlyipToml::read_from_dir(path).unwrap_or_else(|_| GlyipToml {
            package: crate::config::PackageConfig {
                name: name.to_string(),
                version: "0.1.0".to_string(),
                edition: "2024".to_string(),
                authors: Vec::new(),
                description: None,
                bin: None,
                lib: None,
            },
            dependencies: BTreeMap::new(),
            dev_dependencies: BTreeMap::new(),
        });

        let mut sub_deps: Vec<(String, Option<String>, Option<PathBuf>)> = Vec::new();
        for (dep_name, dep) in config.all_dependencies() {
            sub_deps.push((
                dep_name.clone(),
                dep.version().map(String::from),
                dep.path().map(PathBuf::from),
            ));
        }

        let locked = LockedCrate {
            name: name.to_string(),
            version: config.package.version.clone(),
            source: CrateSource::Path {
                path: path.to_string_lossy().to_string(),
            },
            dependencies: sub_deps
                .iter()
                .map(|(n, v, _)| (n.clone(), v.clone().unwrap_or_default()))
                .collect(),
        };
        Ok((locked, sub_deps))
    }

    /// Resolve a git dependency (plan §23.2):
    ///
    /// 1. Resolve the [`GitSpec`] to a concrete commit via the [`GitFetcher`].
    /// 2. Fetch/checkout the source into the shared git cache keyed by the
    ///    repository URL, so repeated resolves reuse the clone.
    /// 3. Read the checked-out `Glyip.toml` for the real version and the
    ///    crate's own (transitive) dependencies.
    /// 4. Produce a [`LockedCrate`] with `CrateSource::Git` recording the URL
    ///    and resolved commit.
    fn fetch_git_dep(
        &self,
        name: &str,
        url: &str,
        spec: &crate::config::GitSpec,
    ) -> GlyipResult<LockedCrate> {
        let owned_fetcher: Box<dyn GitFetcher> =
            Box::new(GitCommandFetcher::new(global_git_cache_dir()));
        let fetcher: &dyn GitFetcher = self
            .git_fetcher
            .as_ref()
            .map(|b| b.as_ref())
            .unwrap_or(owned_fetcher.as_ref());

        let rev = fetcher.resolve_rev(url, spec)?;

        // Cache checkout keyed by a stable hash of the URL so two deps pointing
        // at the same repo share one clone.
        let repo_dir = global_git_cache_dir().join(repo_cache_name(url));
        std::fs::create_dir_all(&repo_dir)?;
        let checkout = fetcher.fetch(url, &rev, &repo_dir)?;

        let config = GlyipToml::read_from_dir(&checkout).unwrap_or_else(|_| GlyipToml {
            package: crate::config::PackageConfig {
                name: name.to_string(),
                version: rev.clone(),
                edition: "2024".to_string(),
                authors: Vec::new(),
                description: None,
                bin: None,
                lib: None,
            },
            dependencies: BTreeMap::new(),
            dev_dependencies: BTreeMap::new(),
        });

        let mut sub_deps: Vec<(String, Option<String>, Option<PathBuf>)> = Vec::new();
        for (dep_name, dep) in config.all_dependencies() {
            sub_deps.push((
                dep_name.clone(),
                dep.version().map(String::from),
                dep.path().map(PathBuf::from),
            ));
        }

        let (branch, tag) = match spec {
            crate::config::GitSpec::Branch(b) => (Some(b.clone()), None),
            crate::config::GitSpec::Tag(t) => (None, Some(t.clone())),
            _ => (None, None),
        };

        let locked = LockedCrate {
            name: name.to_string(),
            version: config.package.version.clone(),
            source: CrateSource::Git {
                url: url.to_string(),
                rev: Some(rev),
                branch,
                tag,
            },
            dependencies: sub_deps
                .iter()
                .map(|(n, v, _)| (n.clone(), v.clone().unwrap_or_default()))
                .collect(),
        };
        Ok(locked)
    }

    /// Download a crate's source code from the registry.
    ///
    /// Returns the path to the extracted source directory.
    pub fn download_crate(&self, locked: &LockedCrate, cache_dir: &Path) -> GlyipResult<PathBuf> {
        if let Some(ref client) = self.registry_client
            && let CrateSource::Registry { .. } = &locked.source
        {
            let dest = cache_dir.join("registry").join(&locked.name);
            return client.download_crate(&locked.name, &locked.version, &dest);
        }
        Err(GlyipError::RegistryError(format!(
            "no registry client available to download '{}'",
            locked.name
        )))
    }

    /// Detect dependency cycles by checking for back-edges.
    pub fn detect_cycles(&self, lockfile: &Lockfile) -> GlyipResult<()> {
        // Build an adjacency list using owned Strings.
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        for locked in lockfile.crates() {
            let deps: Vec<String> = locked.dependencies.keys().cloned().collect();
            graph.insert(locked.name.clone(), deps);
        }

        // DFS-based cycle detection using owned names.
        let mut white: HashSet<String> = graph.keys().cloned().collect();
        let mut gray: HashSet<String> = HashSet::new();
        let mut black: HashSet<String> = HashSet::new();
        let mut path: Vec<String> = Vec::new();

        let nodes: Vec<String> = graph.keys().cloned().collect();
        for node in nodes {
            if white.contains(&node) {
                Self::dfs_cycle(&node, &graph, &mut white, &mut gray, &mut black, &mut path)?;
            }
        }
        Ok(())
    }

    fn dfs_cycle(
        node: &str,
        graph: &HashMap<String, Vec<String>>,
        white: &mut HashSet<String>,
        gray: &mut HashSet<String>,
        black: &mut HashSet<String>,
        path: &mut Vec<String>,
    ) -> GlyipResult<()> {
        white.remove(node);
        gray.insert(node.to_string());
        path.push(node.to_string());

        if let Some(neighbors) = graph.get(node) {
            for neighbor in neighbors {
                if gray.contains(neighbor) {
                    let cycle_start = path.iter().position(|n| n == neighbor).unwrap_or(0);
                    let cycle: Vec<String> = path[cycle_start..]
                        .iter()
                        .chain(std::iter::once(neighbor))
                        .cloned()
                        .collect();
                    return Err(GlyipError::DependencyCycle(cycle));
                }
                if white.contains(neighbor) {
                    Self::dfs_cycle(neighbor, graph, white, gray, black, path)?;
                }
            }
        }

        path.pop();
        gray.remove(node);
        black.insert(node.to_string());
        Ok(())
    }
}

/// Trait for fetching git repositories and resolving refs to commits.
///
/// Plan §23.2: abstracted so tests can inject a mock that returns
/// deterministic commits and checkouts without touching the network. The
/// production implementation ([`GitCommandFetcher`]) shells out to the system
/// `git`.
pub trait GitFetcher {
    /// Resolve a [`GitSpec`] to a concrete commit hash (or ref) for `url`.
    fn resolve_rev(&self, url: &str, spec: &crate::config::GitSpec) -> GlyipResult<String>;

    /// Ensure `url`@`rev` is checked out into `dest`, returning the checkout
    /// directory. Reuses an existing clone when present.
    fn fetch(&self, url: &str, rev: &str, dest: &Path) -> GlyipResult<PathBuf>;
}

/// Returns the shared git cache directory.
///
/// Honours `GLYIM_GIT_CACHE` if set, otherwise `$HOME/.cache/glyip/git`
/// (falling back to a temp dir if `$HOME` is unavailable).
fn global_git_cache_dir() -> PathBuf {
    if let Ok(p) = std::env::var("GLYIM_GIT_CACHE") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME")
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    PathBuf::from(home).join(".cache").join("glyip").join("git")
}

/// Derive a filesystem-safe directory name for a git URL (stable hash).
fn repo_cache_name(url: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    url.hash(&mut h);
    format!("{:x}", h.finish())
}

/// Production [`GitFetcher`] that drives the system `git` binary.
#[derive(Debug, Clone)]
pub struct GitCommandFetcher {
    /// Shared cache root for cloned repositories.
    pub cache_dir: PathBuf,
}

impl GitCommandFetcher {
    /// Create a fetcher with an explicit cache directory.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    fn git(&self, args: &[&str], cwd: Option<&Path>) -> GlyipResult<std::process::Output> {
        let mut cmd = std::process::Command::new("git");
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let output = cmd
            .args(args)
            .output()
            .map_err(|e| GlyipError::RegistryError(format!("failed to invoke git: {e}")))?;
        if !output.status.success() {
            return Err(GlyipError::RegistryError(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(output)
    }
}

impl GitFetcher for GitCommandFetcher {
    fn resolve_rev(&self, url: &str, spec: &crate::config::GitSpec) -> GlyipResult<String> {
        let refspec = match spec {
            crate::config::GitSpec::Branch(b) => format!("refs/heads/{b}"),
            crate::config::GitSpec::Tag(t) => format!("refs/tags/{t}"),
            // For an exact rev or default branch, resolve after clone.
            crate::config::GitSpec::Rev(_) | crate::config::GitSpec::DefaultBranch => {
                return Ok(match spec {
                    crate::config::GitSpec::Rev(r) => r.clone(),
                    _ => "HEAD".to_string(),
                });
            }
        };
        // `git ls-remote` resolves the ref to a commit without cloning.
        let output = self.git(&["ls-remote", url, &refspec], None)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let first_line = stdout.lines().next().ok_or_else(|| {
            GlyipError::RegistryError(format!("git ls-remote {url} returned no refs"))
        })?;
        let commit = first_line.split_whitespace().next().ok_or_else(|| {
            GlyipError::RegistryError(format!("malformed ls-remote output: {first_line}"))
        })?;
        Ok(commit.to_string())
    }

    fn fetch(&self, url: &str, rev: &str, dest: &Path) -> GlyipResult<PathBuf> {
        let clone_target = dest.join(repo_cache_name(url));
        if clone_target.join(".git").exists() {
            // Existing clone: fetch latest and checkout the requested rev.
            self.git(&["fetch", "--all"], Some(&clone_target))?;
        } else {
            std::fs::create_dir_all(&clone_target)?;
            self.git(
                &[
                    "clone",
                    "--no-checkout",
                    url,
                    clone_target.to_str().unwrap(),
                ],
                None,
            )?;
        }
        // Resolve a symbolic rev (e.g. HEAD or a branch name) to a concrete commit.
        let resolved = if rev == "HEAD" {
            let out = self.git(&["rev-parse", "HEAD"], Some(&clone_target))?;
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            rev.to_string()
        };
        self.git(&["checkout", &resolved], Some(&clone_target))?;
        Ok(clone_target)
    }
}
