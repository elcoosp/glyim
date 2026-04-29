use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ── [package] ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[derive(Default)]
pub struct Package {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub edition: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

// ── [dependencies], [macros], [dev-dependencies] ───────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dependency {
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub registry: Option<String>,
    #[serde(default)]
    pub workspace: bool,
    #[serde(default)]
    pub is_macro: bool,
}

// ── [target.*] ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TargetConfig {
    #[serde(default)]
    pub linker: Option<String>,
    #[serde(default)]
    pub sysroot: Option<PathBuf>,
}

// ── [cache] ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheConfig {
    pub remote: String,
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default)]
    pub push: bool,
    #[serde(default)]
    pub pull: bool,
}


// ── [features] ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[derive(Default)]
pub struct FeaturesConfig {
    #[serde(default)]
    pub features: HashMap<String, Vec<String>>,
}

// ── [workspace] ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub dependencies: HashMap<String, Dependency>,
}

// ── Top-level manifest ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageManifest {
    #[serde(default)]
    pub package: Package,
    #[serde(default)]
    pub dependencies: HashMap<String, Dependency>,
    #[serde(default)]
    pub macros: HashMap<String, Dependency>,
    #[serde(default)]
    pub dev_dependencies: HashMap<String, Dependency>,
    #[serde(default)]
    pub target: HashMap<String, TargetConfig>,
    #[serde(default)]
    pub cache: Option<CacheConfig>,
    #[serde(default)]
    pub features: FeaturesConfig,
    #[serde(default)]
    pub workspace: Option<Workspace>,
}

// ── Parsing ──────────────────────────────────────────────────────────────

/// Parse a glyim.toml string into a PackageManifest.
pub fn parse_manifest(toml_str: &str, file_name: &str) -> Result<PackageManifest, crate::PkgError> {
    let manifest: PackageManifest = toml::from_str(toml_str).map_err(|e| {
        crate::PkgError::ParseToml {
            file: file_name.to_string(),
            reason: e.to_string(),
        }
    })?;

    // Validate required fields
    if manifest.package.name.is_empty() {
        return Err(crate::PkgError::MissingField {
            field: "package.name",
            file: file_name.to_string(),
        });
    }

    Ok(manifest)
}

/// Load a PackageManifest from a file path.
pub fn load_manifest(path: &std::path::Path) -> Result<PackageManifest, crate::PkgError> {
    let content = std::fs::read_to_string(path)?;
    let file_name = path.to_string_lossy().to_string();
    parse_manifest(&content, &file_name)
}
