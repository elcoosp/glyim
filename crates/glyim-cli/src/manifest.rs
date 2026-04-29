//! Package manifest parsing — re-exports from glyim-pkg for full spec sections.
//!
//! The legacy SimpleManifest type is available for backward compatibility.

use std::fmt;
use std::path::PathBuf;

// ── Legacy type (backward compat) ────────────────────────────────────────

/// Simplified manifest with just name and version.
/// Used internally where full manifest fields aren't needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleManifest {
    pub name: String,
    pub version: String,
}

impl From<glyim_pkg::manifest::PackageManifest> for SimpleManifest {
    fn from(m: glyim_pkg::manifest::PackageManifest) -> Self {
        SimpleManifest {
            name: m.package.name,
            version: m.package.version,
        }
    }
}

impl fmt::Display for SimpleManifest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} v{}", self.name, self.version)
    }
}

/// Parse a glyim.toml string into a SimpleManifest.
pub fn parse_manifest(toml_str: &str) -> Result<SimpleManifest, ManifestError> {
    let full = glyim_pkg::manifest::parse_manifest(toml_str, "glyim.toml")?;
    Ok(SimpleManifest::from(full))
}

/// Legacy error type for backward compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    FileNotFound(PathBuf),
    Parse(String),
    MissingSection(&'static str),
    MissingField(&'static str),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileNotFound(p) => write!(f, "glyim.toml not found at {}", p.display()),
            Self::Parse(msg) => write!(f, "invalid TOML in glyim.toml: {msg}"),
            Self::MissingSection(s) => write!(f, "missing [{s}] section in glyim.toml"),
            Self::MissingField(fld) => write!(f, "missing required field '{fld}' in glyim.toml"),
        }
    }
}

impl std::error::Error for ManifestError {}

impl From<glyim_pkg::error::PkgError> for ManifestError {
    fn from(e: glyim_pkg::error::PkgError) -> Self {
        match e {
            glyim_pkg::error::PkgError::ParseToml { reason, .. } => ManifestError::Parse(reason),
            glyim_pkg::error::PkgError::MissingSection { section, .. } => ManifestError::MissingSection(section),
            glyim_pkg::error::PkgError::MissingField { field, .. } => ManifestError::MissingField(field),
            _ => ManifestError::Parse(e.to_string()),
        }
    }
}
