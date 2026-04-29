use crate::error::PkgError;

/// Registry client for the Glyim package registry.
///
/// Currently a stub — all methods return errors. Will be implemented in Phase 6 with reqwest.
pub struct RegistryClient {
#[allow(dead_code)]
    endpoint: String,
}

impl RegistryClient {
    pub fn new(base_url: &str) -> Result<Self, PkgError> {
        let url = base_url.trim_end_matches('/').to_string();
        Ok(Self { endpoint: url })
    }

    /// Fetch available versions for a package from the registry.
    pub fn fetch_available(&self, _name: &str) -> Result<Vec<crate::resolver::AvailableVersion>, PkgError> {
        Err(PkgError::Registry("registry client not yet implemented".into()))
    }

    /// Download a package and return its content hash.
    pub fn download(&self, _name: &str, _version: &str, _dest: &std::path::Path) -> Result<String, PkgError> {
        Err(PkgError::Registry("download not yet implemented".into()))
    }

    /// Publish a package to the registry.
    pub fn publish(&self, _archive_path: &std::path::Path) -> Result<(), PkgError> {
        Err(PkgError::Registry("publish not yet implemented".into()))
    }
}
