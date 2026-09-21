//! Lockfile (Glyip.lock) types and serialisation.
//!
//! The lockfile records the exact versions and sources of every resolved
//! dependency so that builds are reproducible.

use crate::error::{GlyipError, GlyipResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const LOCKFILE_NAME: &str = "Glyip.lock";

/// A conflict between the resolved lockfile and the current manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockConflict {
    /// A manifest dependency is missing entirely from the lockfile.
    /// Struct.
    Missing {
        /// name field.
        name: String,
        /// version field.
        version: String,
    },
    /// A manifest dependency's pinned version does not match the locked one.
    VersionMismatch {
        /// Struct.
        name: String,
        /// Struct.
        manifest_version: String,
        /// Struct.
        locked_version: String,
    },
}

impl std::fmt::Display for LockConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockConflict::Missing { name, version } => {
                write!(f, "dependency `{name}@{version}` is not locked")
            }
            LockConflict::VersionMismatch {
                name,
                manifest_version,
                locked_version,
            } => write!(
                f,
                "dependency `{name}` manifest version `{manifest_version}` != locked `{locked_version}`"
            ),
        }
    }
}

/// The source of a crate — where it was fetched from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "source")]
pub enum CrateSource {
    /// A local filesystem path.
    /// Struct.
    Path {
        /// path field.
        path: String,
    },
    /// A git repository at a specific revision.
    Git {
        /// Struct.
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        /// Struct.
        rev: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        /// Struct.
        branch: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        /// Struct.
        tag: Option<String>,
    },
    /// A registry (index) entry.
    /// Struct.
    Registry {
        /// url field.
        url: String,
        /// checksum field.
        checksum: String,
    },
}

/// A single locked dependency entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedCrate {
    /// Crate name.
    pub name: String,
    /// Exact version string.
    pub version: String,
    /// Where the crate came from.
    pub source: CrateSource,
    /// Dependencies of this crate (name → version).
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}

/// The full lockfile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Lockfile {
    /// Lockfile format version.
    pub version: u32,
    /// Locked crates keyed by `name-version`.
    #[serde(default)]
    pub crates: BTreeMap<String, LockedCrate>,
}

impl Lockfile {
    /// Create a new empty lockfile (format version 1).
    pub fn new() -> Self {
        Self {
            version: 1,
            crates: BTreeMap::new(),
        }
    }

    /// Read a lockfile from the given project directory.
    pub fn read_from_dir(dir: &Path) -> GlyipResult<Self> {
        let path = dir.join(LOCKFILE_NAME);
        if !path.exists() {
            return Ok(Self::new());
        }
        let content = std::fs::read_to_string(&path)?;
        Self::parse(&content)
    }

    /// Parse lockfile content from a string.
    pub fn parse(content: &str) -> GlyipResult<Self> {
        toml::from_str(content).map_err(|e| GlyipError::ConfigParse(e.to_string()))
    }

    /// Write the lockfile to the given project directory.
    pub fn write_to_dir(&self, dir: &Path) -> GlyipResult<()> {
        let path = dir.join(LOCKFILE_NAME);
        let content =
            toml::to_string_pretty(self).map_err(|e| GlyipError::ConfigParse(e.to_string()))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Add a locked crate entry.
    pub fn add_crate(&mut self, krate: LockedCrate) {
        let key = format!("{}-{}", krate.name, krate.version);
        self.crates.insert(key, krate);
    }

    /// Look up a locked crate by name and version.
    pub fn get_crate(&self, name: &str, version: &str) -> Option<&LockedCrate> {
        let key = format!("{}-{}", name, version);
        self.crates.get(&key)
    }

    /// Iterate over all locked crates.
    pub fn crates(&self) -> impl Iterator<Item = &LockedCrate> {
        self.crates.values()
    }

    /// Number of locked crates.
    pub fn len(&self) -> usize {
        self.crates.len()
    }

    /// Whether the lockfile contains no crates.
    pub fn is_empty(&self) -> bool {
        self.crates.is_empty()
    }

    /// Plan §23.4: validate the lockfile against the current manifest. Every
    /// manifest dependency (runtime + dev) must have a corresponding locked
    /// entry whose version matches the manifest's pinned version (when the
    /// manifest pins an exact version; otherwise presence alone is required).
    /// Returns the list of conflicts — empty means the lockfile is consistent.
    pub fn validate_against_manifest(
        &self,
        manifest: &crate::config::GlyipToml,
    ) -> Vec<LockConflict> {
        let mut conflicts = Vec::new();
        for (name, dep) in manifest.all_dependencies() {
            match dep.version() {
                Some(manifest_version) => {
                    match self.get_crate(name, manifest_version) {
                        Some(_) => {}
                        None => {
                            // Either missing entirely, or a different version is locked.
                            let locked = self
                                .crates
                                .values()
                                .find(|c| c.name == *name)
                                .map(|c| c.version.clone());
                            match locked {
                                Some(locked_version) => {
                                    conflicts.push(LockConflict::VersionMismatch {
                                        name: name.clone(),
                                        manifest_version: manifest_version.to_string(),
                                        locked_version,
                                    })
                                }
                                None => conflicts.push(LockConflict::Missing {
                                    name: name.clone(),
                                    version: manifest_version.to_string(),
                                }),
                            }
                        }
                    }
                }
                None => {
                    // No exact version pinned — require at least one locked entry.
                    if !self.crates.values().any(|c| c.name == *name) {
                        conflicts.push(LockConflict::Missing {
                            name: name.clone(),
                            version: "*".to_string(),
                        });
                    }
                }
            }
        }
        conflicts
    }
}

impl Default for Lockfile {
    fn default() -> Self {
        Self::new()
    }
}
