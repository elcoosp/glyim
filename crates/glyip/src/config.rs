//! Configuration file (Glyip.toml) parsing and types.

use crate::error::{GlyipError, GlyipResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const GLYIP_TOML: &str = "Glyip.toml";

fn default_edition() -> String {
    "2024".to_string()
}

fn default_version() -> String {
    "0.1.0".to_string()
}

/// Root configuration for a Glyim project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GlyipToml {
    /// Package metadata.
    pub package: PackageConfig,
    /// Runtime dependencies.
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    /// Development-only dependencies.
    #[serde(default)]
    pub dev_dependencies: BTreeMap<String, Dependency>,
}

/// Package metadata section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PackageConfig {
    /// Crate name.
    pub name: String,
    /// Semantic version.
    #[serde(default = "default_version")]
    pub version: String,
    /// Language edition.
    #[serde(default = "default_edition")]
    pub edition: String,
    /// Author list.
    #[serde(default)]
    pub authors: Vec<String>,
    /// Short description.
    #[serde(default)]
    pub description: Option<String>,
    /// Binary targets.
    #[serde(default)]
    pub bin: Option<Vec<BinTarget>>,
    /// Library target.
    #[serde(default)]
    pub lib: Option<LibTarget>,
}

/// A binary target inside the package.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BinTarget {
    /// Binary name.
    pub name: String,
    /// Source file path (relative to project root).
    #[serde(default)]
    pub path: Option<PathBuf>,
}

/// A library target inside the package.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LibTarget {
    /// Source file path (relative to project root).
    #[serde(default)]
    pub path: Option<PathBuf>,
}

/// A git dependency specification — which ref of a repository to pin.
///
/// Plan §23.2: supports `branch`, `tag`, exact `rev`, or the repository's
/// default branch. Exactly one is active; `DefaultBranch` is used when none of
/// the others is supplied.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GitSpec {
    /// Track a named branch (resolved to its current tip commit).
    Branch(String),
    /// Track a named tag (resolved to the commit the tag points at).
    Tag(String),
    /// Pin an exact revision hash (must exist in the repository).
    Rev(String),
    /// Use the repository's default branch tip.
    DefaultBranch,
}

impl GitSpec {
    /// Human-readable label used for diagnostics and lockfile round-tripping.
    pub fn label(&self) -> String {
        match self {
            GitSpec::Branch(b) => format!("branch {b}"),
            GitSpec::Tag(t) => format!("tag {t}"),
            GitSpec::Rev(r) => format!("rev {r}"),
            GitSpec::DefaultBranch => "default branch".to_string(),
        }
    }
}

/// A dependency specification — either a bare version string or a detailed table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Dependency {
    /// Simple version requirement: `foo = "1.0"`.
    Simple(String),
    /// Detailed dependency: `foo = { version = "1.0", path = "../foo" }`.
    Detailed(DependencyDetail),
}

/// Detailed dependency configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DependencyDetail {
    /// Version requirement (semver-like).
    #[serde(default)]
    pub version: Option<String>,
    /// Local filesystem path.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Git repository URL.
    #[serde(default)]
    pub git: Option<String>,
    /// Git branch.
    #[serde(default)]
    pub branch: Option<String>,
    /// Git tag.
    #[serde(default)]
    pub tag: Option<String>,
    /// Git revision hash.
    #[serde(default)]
    pub rev: Option<String>,
}

impl Dependency {
    /// Return the version requirement, if any.
    pub fn version(&self) -> Option<&str> {
        match self {
            Dependency::Simple(v) => Some(v),
            Dependency::Detailed(d) => d.version.as_deref(),
        }
    }

    /// Return the local path, if any.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Dependency::Simple(_) => None,
            Dependency::Detailed(d) => d.path.as_deref(),
        }
    }

    /// Return the git URL, if any.
    pub fn git(&self) -> Option<&str> {
        match self {
            Dependency::Simple(_) => None,
            Dependency::Detailed(d) => d.git.as_deref(),
        }
    }

    /// Return the resolved [`GitSpec`] if this is a git dependency, or `None`.
    ///
    /// Plan §23.2: `rev` takes precedence over `tag` over `branch`; if none are
    /// set but a `git` URL is present, the default branch is used.
    pub fn git_spec(&self) -> Option<GitSpec> {
        match self {
            Dependency::Simple(_) => None,
            Dependency::Detailed(d) => {
                let git = d.git.as_deref()?;
                if git.is_empty() {
                    return None;
                }
                let spec = if let Some(rev) = &d.rev {
                    GitSpec::Rev(rev.clone())
                } else if let Some(tag) = &d.tag {
                    GitSpec::Tag(tag.clone())
                } else if let Some(branch) = &d.branch {
                    GitSpec::Branch(branch.clone())
                } else {
                    GitSpec::DefaultBranch
                };
                Some(spec)
            }
        }
    }
}

impl GlyipToml {
    /// Read a Glyip.toml from the given directory.
    pub fn read_from_dir(dir: &Path) -> GlyipResult<Self> {
        let path = dir.join(GLYIP_TOML);
        if !path.exists() {
            return Err(GlyipError::ProjectNotFound(dir.to_path_buf()));
        }
        let content = std::fs::read_to_string(&path)?;
        Self::parse(&content)
    }

    /// Parse Glyip.toml content from a string.
    pub fn parse(content: &str) -> GlyipResult<Self> {
        toml::from_str(content).map_err(|e| GlyipError::ConfigParse(e.to_string()))
    }

    /// Write the configuration to a directory.
    pub fn write_to_dir(&self, dir: &Path) -> GlyipResult<()> {
        let path = dir.join(GLYIP_TOML);
        let content =
            toml::to_string_pretty(self).map_err(|e| GlyipError::ConfigParse(e.to_string()))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get the project name.
    pub fn name(&self) -> &str {
        &self.package.name
    }

    /// Iterate over all dependencies (runtime + dev).
    pub fn all_dependencies(&self) -> impl Iterator<Item = (&String, &Dependency)> {
        self.dependencies.iter().chain(self.dev_dependencies.iter())
    }
}

/// Options for the `glyip new` command.
#[derive(Debug, Clone)]
pub struct NewOptions {
    /// Create a library project instead of a binary.
    pub lib: bool,
    /// Language edition.
    pub edition: String,
}

impl Default for NewOptions {
    fn default() -> Self {
        Self {
            lib: false,
            edition: "2024".to_string(),
        }
    }
}

/// Options for the `glyip build` command.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Build in release mode.
    pub release: bool,
    /// Target triple.
    pub target: Option<String>,
    /// Codegen backend name.
    pub backend: String,
    /// Optimisation level (0–3).
    pub opt_level: u8,
    /// Link-time optimization strategy (Phase 10.2): `"off"`, `"thin"`, or
    /// `"fat"`. Mapped onto `glyim_codegen_llvm::passes::LtoKind` at the
    /// backend boundary. `None`/`off` is the default.
    pub lto: Option<glyim_codegen_llvm::passes::LtoKind>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            release: false,
            target: None,
            backend: "llvm".to_string(),
            opt_level: 0,
            lto: None,
        }
    }
}

/// Options for the `glyip test` command.
#[derive(Debug, Clone, Default)]
pub struct TestOptions {
    /// Build in release mode.
    pub release: bool,
    /// Only run tests whose name contains this substring.
    pub filter: Option<String>,
    /// Compile tests but do not run them.
    pub no_run: bool,
    /// Run tests annotated with `#[ignore]` (mirrors `cargo test -- --ignored`).
    pub run_ignored: bool,
    /// Plan §23.1: compile each test file to a native executable and run it as
    /// a real subprocess (instead of the in-process MIR interpreter). This
    /// exercises actual compiled-binary behavior (panics/aborts/FFI/exit codes)
    /// and is the preferred "real" execution path.
    pub compiled: bool,
}

/// Options for the `glyip run` command.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Build in release mode.
    pub release: bool,
    /// Arguments to pass to the binary.
    pub args: Vec<String>,
    /// Codegen backend name.
    pub backend: String,
    /// Target triple. When `None`, the host/default triple is used. (Plan §21.3:
    /// `glyip run` must honour `--target` rather than silently dropping it.)
    pub target: Option<String>,
    /// Link-time optimization strategy (Phase 10.2), forwarded to the build
    /// step. `None`/`off` is the default.
    pub lto: Option<glyim_codegen_llvm::passes::LtoKind>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            release: false,
            args: Vec::new(),
            backend: "llvm".to_string(),
            target: None,
            lto: None,
        }
    }
}
