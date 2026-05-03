//! Remote ContentStore implementation.
//!
//! Talks to a CAS server over HTTP. Falls back to a local store for
//! already-fetched artifacts.

use crate::hash::ContentHash;
use crate::local::LocalContentStore;
use crate::store::{ActionResult, ContentStore, StoreError};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct RemoteStoreConfig {
    pub endpoint: String,
    pub auth_token: Option<String>,
    pub local_dir: PathBuf,
}

pub struct RemoteContentStore {
    endpoint: String,
    auth_token: Option<String>,
    client: reqwest::blocking::Client,
    local: LocalContentStore,
}

impl RemoteContentStore {
    fn remote_store_action_result(
        &self,
        hash: ContentHash,
        result: &ActionResult,
    ) -> Result<(), StoreError> {
        let url = format!("{}/action/{}", self.endpoint, hash);
        let response = self
            .client
            .post(&url)
            .headers(self.request_headers())
            .json(result)
            .send()
            .map_err(|e| StoreError::Network(format!("remote store action: {e}")))?;
        if !response.status().is_success() {
            return Err(StoreError::Network(format!(
                "remote store action returned {}",
                response.status()
            )));
        }
        Ok(())
    }

    fn remote_retrieve_action_result(
        &self,
        hash: ContentHash,
    ) -> Result<Option<ActionResult>, StoreError> {
        let url = format!("{}/action/{}", self.endpoint, hash);
        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|e| StoreError::Network(format!("remote retrieve action: {e}")))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(StoreError::Network(format!(
                "remote retrieve action returned {}",
                response.status()
            )));
        }
        let result: ActionResult = response
            .json()
            .map_err(|e| StoreError::Network(format!("deserialize action: {e}")))?;
        Ok(Some(result))
    }

    pub fn new(config: &RemoteStoreConfig) -> Result<Self, StoreError> {
        let local = LocalContentStore::new(&config.local_dir)?;
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| StoreError::Network(e.to_string()))?;
        Ok(Self {
            endpoint: config.endpoint.trim_end_matches('/').to_string(),
            auth_token: config.auth_token.clone(),
            client,
            local,
        })
    }

    fn request_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Content-Type", "application/octet-stream".parse().unwrap());
        if let Some(token) = &self.auth_token {
            headers.insert("Authorization", format!("Bearer {token}").parse().unwrap());
        }
        headers
    }

    fn remote_store_blob(&self, content: &[u8]) -> Result<ContentHash, StoreError> {
        let hash = ContentHash::of(content);
        let url = format!("{}/blob", self.endpoint);
        let response = self
            .client
            .post(&url)
            .headers(self.request_headers())
            .body(content.to_vec())
            .send()
            .map_err(|e| StoreError::Network(e.to_string()))?;
        if !response.status().is_success() {
            return Err(StoreError::Network(format!(
                "remote store returned {}",
                response.status()
            )));
        }
        Ok(hash)
    }

    fn remote_retrieve_blob(&self, hash: ContentHash) -> Result<Option<Vec<u8>>, StoreError> {
        let url = format!("{}/blob/{}", self.endpoint, hash);
        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|e| StoreError::Network(e.to_string()))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(StoreError::Network(format!(
                "remote retrieve returned {}",
                response.status()
            )));
        }
        let bytes = response
            .bytes()
            .map_err(|e| StoreError::Network(e.to_string()))?;
        Ok(Some(bytes.to_vec()))
    }

    fn remote_has_blobs(&self, hashes: &[ContentHash]) -> Result<Vec<ContentHash>, StoreError> {
        let url = format!("{}/blob/missing", self.endpoint);
        let hash_list: Vec<String> = hashes.iter().map(|h| h.to_string()).collect();
        let response = self
            .client
            .post(&url)
            .json(&hash_list)
            .send()
            .map_err(|e| StoreError::Network(e.to_string()))?;
        if !response.status().is_success() {
            // If the endpoint doesn't exist, treat all as missing
            return Ok(hashes.to_vec());
        }
        let missing: Vec<String> = response.json().unwrap_or_else(|_| hash_list.clone());
        Ok(missing
            .iter()
            .filter_map(|h| h.parse::<ContentHash>().ok())
            .collect())
    }
}

impl ContentStore for RemoteContentStore {
    fn store(&self, content: &[u8]) -> ContentHash {
        let hash = ContentHash::of(content);
        // Always store locally
        self.local.store(content);
        // Best-effort remote push
        let _ = self.remote_store_blob(content);
        hash
    }

    fn retrieve(&self, hash: ContentHash) -> Option<Vec<u8>> {
        // Check local first
        if let Some(data) = self.local.retrieve(hash) {
            return Some(data);
        }
        // Try remote
        match self.remote_retrieve_blob(hash) {
            Ok(Some(data)) => {
                // Cache locally
                self.local.store(&data);
                Some(data)
            }
            Ok(None) => None,
            Err(_) => None,
        }
    }

    fn register_name(&self, name: &str, hash: ContentHash) {
        self.local.register_name(name, hash);
    }

    fn resolve_name(&self, name: &str) -> Option<ContentHash> {
        self.local.resolve_name(name)
    }

    fn store_action_result(
        &self,
        action_hash: ContentHash,
        result: ActionResult,
    ) -> Result<(), StoreError> {
        // Store locally always
        let _ = ContentStore::store_action_result(&self.local, action_hash, result.clone());
        // Best-effort remote via action endpoint
        let _ = self.remote_store_action_result(action_hash, &result);
        Ok(())
    }

    fn retrieve_action_result(&self, action_hash: ContentHash) -> Option<ActionResult> {
        // Check local first
        if let Some(result) = ContentStore::retrieve_action_result(&self.local, action_hash) {
            return Some(result);
        }
        // Check remote action cache
        match self.remote_retrieve_action_result(action_hash) {
            Ok(Some(result)) => {
                // Cache locally
                let _ = ContentStore::store_action_result(&self.local, action_hash, result.clone());
                Some(result)
            }
            _ => None,
        }
    }

    fn has_blobs(&self, hashes: &[ContentHash]) -> Vec<ContentHash> {
        // Which are available locally?
        let available: Vec<ContentHash> = hashes
            .iter()
            .filter(|h| self.local.retrieve(**h).is_some())
            .copied()
            .collect();
        if available.len() == hashes.len() {
            return vec![];
        }
        let missing: Vec<ContentHash> = hashes
            .iter()
            .filter(|h| !available.contains(h))
            .copied()
            .collect();
        // Ask remote about the missing ones
        match self.remote_has_blobs(&missing) {
            Ok(remote_missing) => remote_missing,
            Err(_) => missing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_store_new_with_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = RemoteStoreConfig {
            endpoint: "http://localhost:9090".to_string(),
            auth_token: None,
            local_dir: dir.path().to_path_buf(),
        };
        let store = RemoteContentStore::new(&config);
        assert!(store.is_ok());
    }

    #[test]
    fn remote_store_retrieve_missing_returns_none_on_network_error() {
        let dir = tempfile::tempdir().unwrap();
        let config = RemoteStoreConfig {
            endpoint: "http://localhost:99999".to_string(), // bad port
            auth_token: None,
            local_dir: dir.path().to_path_buf(),
        };
        let store = RemoteContentStore::new(&config).unwrap();
        let result = store.retrieve(ContentHash::of_str("nonexistent"));
        assert!(result.is_none());
    }
}
