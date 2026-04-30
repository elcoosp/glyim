use crate::error::PkgError;
use crate::lockfile::LockSource;
use crate::resolver::AvailableVersion;
use serde::{Deserialize, Serialize};

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
        Ok(Self { endpoint: url, client })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn fetch_available(&self, name: &str) -> Result<Vec<AvailableVersion>, PkgError> {
        let url = format!("{}/api/v1/packages/{}", self.endpoint, name);
        let response = self.client.get(&url).send()
            .map_err(|e| PkgError::Registry(format!("fetch {name}: {e}")))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(PkgError::Registry(format!("package '{}' not found in registry", name)));
        }
        if !response.status().is_success() {
            return Err(PkgError::Registry(format!("registry returned {} for package '{}'", response.status(), name)));
        }

        let meta: RegistryPackageMeta = response.json()
            .map_err(|e| PkgError::Registry(format!("parse response for {name}: {e}")))?;

        let versions: Vec<AvailableVersion> = meta.versions.into_iter().map(|entry| {
            let deps: Vec<crate::resolver::Requirement> = entry.deps.iter().map(|d| crate::resolver::Requirement {
                name: d.name.clone(),
                version_constraint: d.version.clone(),
                is_macro: d.is_macro,
                source: LockSource::Registry { url: self.endpoint.clone() },
            }).collect();
            AvailableVersion {
                version: entry.version,
                is_macro: entry.is_macro,
                deps,
                source: LockSource::Registry { url: self.endpoint.clone() },
            }
        }).collect();

        Ok(versions)
    }

    pub fn publish(&self, name: &str, version: &str, archive_data: &[u8]) -> Result<(), PkgError> {
        let url = format!("{}/api/v1/packages/{}/{}/upload", self.endpoint, name, version);
        let response = self.client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(archive_data.to_vec())
            .send()
            .map_err(|e| PkgError::Registry(format!("publish upload: {e}")))?;
        if !response.status().is_success() {
            return Err(PkgError::Registry(format!("publish returned {}", response.status())));
        }
        Ok(())
    }

    pub fn get_latest_version(&self, name: &str) -> Result<Option<String>, PkgError> {
        let versions = self.fetch_available(name)?;
        let latest = versions.iter()
            .filter(|v| !v.is_macro)
            .map(|v| v.version.clone())
            .last();
        Ok(latest)
    }
}
