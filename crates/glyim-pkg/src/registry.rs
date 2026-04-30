use crate::error::PkgError;
use crate::lockfile::LockSource;
use crate::resolver::AvailableVersion;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryPackageMeta {
    name: String,
    versions: Vec<RegistryVersionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryVersionEntry {
    version: String,
    #[serde(default)]
    is_macro: bool,
    #[serde(default)]
    deps: Vec<RegistryDepEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryDepEntry {
    name: String,
    version: String,
    #[serde(default)]
    is_macro: bool,
}

/// Registry client for the Glyim package registry.
pub struct RegistryClient {
    endpoint: String,
    client: reqwest::blocking::Client,
}

impl RegistryClient {
    pub fn new(base_url: &str) -> Result<Self, PkgError> {
        let url = base_url.trim_end_matches('/').to_string();
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| PkgError::Registry(format!("create HTTP client: {e}")))?;
        Ok(Self {
            endpoint: url,
            client,
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Fetch available versions for a package from the registry.
    pub fn fetch_available(&self, name: &str) -> Result<Vec<AvailableVersion>, PkgError> {
        let url = format!("{}/api/v1/packages/{}", self.endpoint, name);
        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|e| PkgError::Registry(format!("fetch {name}: {e}")))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(PkgError::Registry(format!(
                "package '{}' not found in registry",
                name
            )));
        }
        if !response.status().is_success() {
            return Err(PkgError::Registry(format!(
                "registry returned {} for package '{}'",
                response.status(),
                name
            )));
        }

        let meta: RegistryPackageMeta = response
            .json()
            .map_err(|e| PkgError::Registry(format!("parse response for {name}: {e}")))?;

        let versions: Vec<AvailableVersion> = meta
            .versions
            .into_iter()
            .map(|entry| {
                let deps: Vec<crate::resolver::Requirement> = entry
                    .deps
                    .iter()
                    .map(|d| crate::resolver::Requirement {
                        name: d.name.clone(),
                        version_constraint: d.version.clone(),
                        is_macro: d.is_macro,
                        source: LockSource::Registry {
                            url: self.endpoint.clone(),
                        },
                    })
                    .collect();
                AvailableVersion {
                    version: entry.version,
                    is_macro: entry.is_macro,
                    deps,
                    source: LockSource::Registry {
                        url: self.endpoint.clone(),
                    },
                }
            })
            .collect();

        Ok(versions)
    }

    /// Download a package archive and return its content hash.
    pub fn download(&self, name: &str, version: &str, dest: &Path) -> Result<String, PkgError> {
        let url = format!(
            "{}/api/v1/packages/{}/{}/download",
            self.endpoint, name, version
        );
        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|e| PkgError::Registry(format!("download {name}@{version}: {e}")))?;

        if !response.status().is_success() {
            return Err(PkgError::Registry(format!(
                "download {name}@{version} returned {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .map_err(|e| PkgError::Registry(format!("read response: {e}")))?;

        let hash = crate::lockfile::compute_content_hash(&bytes);

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(PkgError::Io)?;
        }
        std::fs::write(dest, &bytes).map_err(PkgError::Io)?;

        Ok(hash)
    }

    /// Publish a package archive to the registry.
    pub fn publish(&self, _archive_path: &Path) -> Result<(), PkgError> {
        // TODO: Implement multipart upload with auth
        Err(PkgError::Registry("publish not yet implemented".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_client_new_with_url() {
        let client = RegistryClient::new("https://registry.glyim.dev").unwrap();
        assert_eq!(client.endpoint(), "https://registry.glyim.dev");
    }

    #[test]
    fn registry_client_fetch_returns_error_on_bad_url() {
        let client = RegistryClient::new("http://localhost:99999").unwrap();
        let result = client.fetch_available("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn registry_client_download_returns_error_on_bad_url() {
        let client = RegistryClient::new("http://localhost:99999").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let result = client.download("pkg", "1.0.0", dir.path());
        assert!(result.is_err());
    }
}
