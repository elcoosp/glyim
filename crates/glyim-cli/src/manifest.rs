use std::fmt;
use std::path::PathBuf;

/// Parsed contents of a `glyim.toml` [package] section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone)]
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
            Self::MissingSection(section) => write!(f, "missing [{section}] section in glyim.toml"),
            Self::MissingField(field) => write!(f, "missing required field '{field}' in glyim.toml"),
        }
    }
}

impl std::error::Error for ManifestError {}

pub fn parse_manifest(toml_str: &str) -> Result<PackageManifest, ManifestError> {
    let value: toml::Value = toml_str
        .parse()
        .map_err(|e: toml::de::Error| ManifestError::Parse(e.to_string()))?;

    let package = value
        .get("package")
        .ok_or(ManifestError::MissingSection("package"))?;

    let name = package
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or(ManifestError::MissingField("package.name"))?
        .to_string();

    let version = package
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0")
        .to_string();

    Ok(PackageManifest { name, version })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_manifest() {
        let toml = "[package]\nname = \"myapp\"\nversion = \"1.0.0\"\n";
        let m = parse_manifest(toml).unwrap();
        assert_eq!(m.name, "myapp");
        assert_eq!(m.version, "1.0.0");
    }

    #[test]
    fn parse_version_defaults_to_0_0_0() {
        let toml = "[package]\nname = \"myapp\"\n";
        let m = parse_manifest(toml).unwrap();
        assert_eq!(m.name, "myapp");
        assert_eq!(m.version, "0.0.0");
    }

    #[test]
    fn parse_missing_package_section() {
        let toml = "[something]\nname = \"x\"\n";
        let err = parse_manifest(toml).unwrap_err();
        assert!(matches!(err, ManifestError::MissingSection("package")));
    }

    #[test]
    fn parse_missing_name_field() {
        let toml = "[package]\nversion = \"1.0.0\"\n";
        let err = parse_manifest(toml).unwrap_err();
        assert!(matches!(err, ManifestError::MissingField("package.name")));
    }

    #[test]
    fn parse_name_wrong_type() {
        let toml = "[package]\nname = 42\n";
        let err = parse_manifest(toml).unwrap_err();
        assert!(matches!(err, ManifestError::MissingField("package.name")));
    }

    #[test]
    fn parse_malformed_toml() {
        let toml = "[package\nname = \"x\"";
        let err = parse_manifest(toml).unwrap_err();
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn parse_empty_string() {
        let err = parse_manifest("").unwrap_err();
        assert!(matches!(err, ManifestError::MissingSection("package")));
    }

    #[test]
    fn parse_extra_fields_ignored() {
        let toml = "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2024\"\nauthors = [\"me\"]\n";
        let m = parse_manifest(toml).unwrap();
        assert_eq!(m.name, "x");
        assert_eq!(m.version, "0.1.0");
    }
}
