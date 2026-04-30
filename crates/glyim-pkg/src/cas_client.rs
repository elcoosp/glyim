use crate::error::PkgError;
use glyim_macro_vfs::{ContentHash, ContentStore, LocalContentStore, RemoteContentStore};
use std::path::Path;

/// Client for the Glyim content-addressable storage system.
///
/// Uses a local store by default. If configured with a remote endpoint,
/// uses a `RemoteContentStore` that caches fetched artifacts locally.
pub struct CasClient {
    store: Box<dyn ContentStore>,
}

impl CasClient {
    /// Create a client backed only by a local store.
    pub fn new(base_dir: &Path) -> std::io::Result<Self> {
        let local = LocalContentStore::new(base_dir)?;
        Ok(Self {
            store: Box::new(local),
        })
    }

    /// Create a client with remote CAS support.
    /// Falls back to local storage when remote is unavailable.
    pub fn new_with_remote(
        base_dir: &Path,
        remote_url: &str,
        auth_token: Option<&str>,
    ) -> Result<Self, PkgError> {
        let config = glyim_macro_vfs::RemoteStoreConfig {
            endpoint: remote_url.to_string(),
            auth_token: auth_token.map(|s| s.to_string()),
            local_dir: base_dir.to_path_buf(),
        };
        let remote =
            RemoteContentStore::new(&config).map_err(|e| PkgError::Registry(e.to_string()))?;
        Ok(Self {
            store: Box::new(remote),
        })
    }

    /// Store content and return its hash.
    pub fn store(&self, content: &[u8]) -> ContentHash {
        self.store.store(content)
    }

    /// Retrieve content by its hash.
    pub fn retrieve(&self, hash: ContentHash) -> Option<Vec<u8>> {
        self.store.retrieve(hash)
    }

    /// Check if content is available (locally or remotely).
    pub fn contains(&self, hash: ContentHash) -> bool {
        self.retrieve(hash).is_some()
    }

    /// Register a name for a hash.
    pub fn register_name(&self, name: &str, hash: ContentHash) {
        self.store.register_name(name, hash);
    }

    /// Resolve a name to a hash.
    pub fn resolve_name(&self, name: &str) -> Option<ContentHash> {
        self.store.resolve_name(name)
    }

    /// Check which blobs are missing (locally or from remote).
    pub fn has_blobs(&self, hashes: &[ContentHash]) -> Vec<ContentHash> {
        self.store.has_blobs(hashes)
    }

    /// Download content from a remote CAS server (stub).
    pub fn download_from_remote(
        &self,
        _hash: ContentHash,
        _remote_url: &str,
    ) -> Result<(), PkgError> {
        Err(PkgError::Registry("use new_with_remote instead".into()))
    }
}
