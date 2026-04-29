use thiserror::Error;

#[derive(Error, Debug)]
pub enum PkgError {
    #[error("invalid TOML in {file}: {reason}")]
    ParseToml { file: String, reason: String },
    #[error("missing [{section}] section in {file}")]
    MissingSection { section: &'static str, file: String },
    #[error("missing required field '{field}' in {file}")]
    MissingField { field: &'static str, file: String },
    #[error("invalid version '{version}': {reason}")]
    InvalidVersion { version: String, reason: String },
    #[error("dependency resolution failed: {0}")]
    Resolution(String),
    #[error("registry error: {0}")]
    Registry(String),
    #[error("lockfile error: {0}")]
    Lockfile(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("workspace error: {0}")]
    Workspace(String),
}
